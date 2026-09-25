#!/bin/sh
# Runs inside the cang microVM. The nixpkgs `ungoogled-chromium` wrapper from
# the cang image (browserImageLayer) provides LD_LIBRARY_PATH/XDG_DATA_DIRS
# itself, so there is no environment setup here. Evidence is written to
# /workspace/evidence (a host-visible bind) so the host runner can score it.
#
# Guest always exits 0; Chromium's own result is judged from the evidence
# files, keeping the VM exit code and the browser exit code distinct.
set -u
E=/workspace/evidence
mkdir -p "$E"

# The host runner stages the mode next to this script; environment variables do
# not survive into the guest (cang passes only PATH plus its allowlist), so a
# file is the only reliable channel for it.
MODE="$(cat /workspace/smoke/run-mode 2>/dev/null || echo single)"
# venus = cang --gpu=drm (guest Vulkan is the venus ICD); software = no --gpu=drm,
# where guest-init's waypipe path pins the lavapipe ICD instead. The host writes
# this file; the guest has no other channel for it.
RENDERER_MODE="$(cat /workspace/smoke/renderer-mode 2>/dev/null || echo venus)"
if [ "$RENDERER_MODE" = software ]; then
  # A software Vulkan device is on Chromium's GPU blocklist, so WebGL is refused
  # until it is ignored; without that the page only ever reports no-webgl.
  ANGLE_FLAGS="--use-angle=vulkan --ignore-gpu-blocklist"
  PRESENT_LABEL=waypipe-software
else
  ANGLE_FLAGS="--use-angle=vulkan --disable-vulkan-surface"
  PRESENT_LABEL=waypipe-venus
fi
PRESENT_TIMEOUT=180
# How long the presenting Chromium has the guest to itself before the headless
# checks start. The host screenshots the compositor during this window; running
# the two phases concurrently starved the presenting renderer (its window set
# its title but never put pixels on the wire within the capture window).
PRESENT_DWELL="${PRESENT_DWELL:-90}"
PRESENT_LOG="$E/presenting-$MODE.log"
PRESENT_STATE="$E/presenting-$MODE-state.txt"
PRESENT_PID=""

# Presenting run (started before the headless checks so its window is mapped for
# as long as possible; the host screenshots the compositor while this is up).
if [ "$MODE" != "single" ]; then
  # GBM_BACKENDS_PATH is required for hardware acceleration: guest-init points
  # LIBGL/EGL/VK at the image's mesa but never sets the GBM backend path, so
  # ozone searches the NixOS default /run/opengl-driver/lib/gbm, misses the
  # guest's dri_gbm.so and cannot init a DRM render node - the GPU process then
  # degrades to software and every buffer arrives as wl_shm.
  # Only the DRM/venus run has a GBM/DRM render node to find. The software run
  # composites on llvmpipe and must not be pointed at a venus GBM backend.
  [ "$RENDERER_MODE" = software ] || export GBM_BACKENDS_PATH=/usr/lib/cang-mesa-runtime/lib/gbm
  rm -rf /tmp/chromium-smoke-present
  # --use-angle=vulkan sends ANGLE (WebGL/raster) to Vulkan (venus, or lavapipe
  # in the software run). Never add --enable-features=Vulkan: that moves the
  # display compositor onto Vulkan, which needs a VkSurfaceKHR ozone-wayland
  # does not implement, and the GPU process then crash-loops and never paints.
  timeout "$PRESENT_TIMEOUT" chromium --ozone-platform=wayland --no-sandbox \
    --disable-gpu-sandbox $ANGLE_FLAGS \
    --user-data-dir=/tmp/chromium-smoke-present --window-size=640,480 \
    --window-position=0,0 \
    --app="file:///workspace/smoke/waypipe-present.html?label=$PRESENT_LABEL" \
    --enable-logging=stderr > "$PRESENT_LOG" 2>&1 &
  PRESENT_PID=$!
fi

# Let the presenting run own the guest while the host captures frames. The
# headless checks below are deliberately serialised after this: running them
# concurrently starved the presenting renderer.
# Only the run that actually has a waypipe display is worth dwelling on; in the
# control run there is no display, so the browser exits at once and waiting would
# just lengthen the run.
if [ -n "$PRESENT_PID" ] && [ -n "${WAYLAND_DISPLAY:-}" ]; then
  sleep "$PRESENT_DWELL"
fi

# Guest GPU diagnostics: the baseline question is whether the guest even sees a
# DRM render node and the venus Vulkan ICD. Unscored, but the first thing to
# read when the WebGL check fails.
{
  echo "uname: $(uname -a)"
  echo "--- /dev/dri ---"
  ls -l /dev/dri 2>&1
  echo "--- virtio devices ---"
  ls /sys/bus/virtio/devices 2>&1
  echo "--- gpu-related env ---"
  env | grep -iE 'VK_|CANG_GPU|LIBGL|MESA|EGL|GALLIUM|DRM' 2>&1
  echo "--- vulkan icd.d dirs ---"
  for d in /run/opengl-driver/share/vulkan/icd.d /share/vulkan/icd.d; do
    echo "[$d]"; ls -l "$d" 2>&1
  done
  echo "--- vulkaninfo ---"
  if command -v vulkaninfo >/dev/null 2>&1; then
    vulkaninfo --summary 2>&1 | head -40
  else
    echo "vulkaninfo not present"
  fi
} > "$E/gpu-diag.txt" 2>&1

# The pin under test in the software run lives in the waypipe process tree:
# guest-init exports the software-renderer environment to the waypipe server and
# its command child (see the waypipe design doc), and that tree is what the
# presenting run is spawned from. Read it off that process instead of this
# script's own environment. It must name the lavapipe ICD through
# VK_ICD_FILENAMES and never VK_DRIVER_FILES -- the Vulkan loader gives
# VK_DRIVER_FILES precedence over the VK_ICD_FILENAMES a client (ANGLE's
# SwiftShader display, for one) sets for itself. Unscored in the venus run,
# scored in the software run.
WP_PID=""
for p in /proc/[0-9]*; do
  cl="$(tr '\0' ' ' < "$p/cmdline" 2>/dev/null)"
  case "$cl" in
    *waypipe*server*) WP_PID="${p#/proc/}" ;;
  esac
done
{
  printf 'renderer-mode=%s\n' "$RENDERER_MODE"
  printf 'waypipe-server-pid=%s\n' "${WP_PID:-<none>}"
  for v in LIBGL_ALWAYS_SOFTWARE LIBGL_DRIVERS_PATH __EGL_VENDOR_LIBRARY_FILENAMES VK_ICD_FILENAMES VK_DRIVER_FILES; do
    value=""
    [ -n "$WP_PID" ] && value="$(tr '\0' '\n' < "/proc/$WP_PID/environ" 2>/dev/null | sed -n "s/^$v=//p")"
    printf '%s=%s\n' "$v" "${value:-<unset>}"
  done
} > "$E/renderer-env-$MODE.txt" 2>&1
# Unscored diagnostics: this script's own environment, for comparison.
env | sort > "$E/guest-env-$MODE.txt" 2>&1

# Every Chromium invocation is bounded: the venus/render-server path being
# exercised is known to stall, and a stall must be attributed to one run
# instead of hanging until the host-side VM timeout kills everything.
CHROME_TIMEOUT=150

: > "$E/version.txt"
chromium --version > "$E/version.txt" 2>&1 || true

# --disable-vulkan-surface is required in this environment: ANGLE's Vulkan
# WSI/swapchain path never completes over venus. The GPU process drives venus
# fine (dozens of DRM_IOCTL_VIRTGPU_EXECBUFFER on /dev/dri/renderD128 all return
# 0), then goes quiet for ~6s and aborts (SIGABRT, so the browser reports
# "GPU process exited unexpectedly: exit_code=6") with renderer= empty. With the
# Vulkan surface disabled ANGLE takes a non-WSI path and the guest reports the
# hardware venus renderer.
#
# The software run passes --ignore-gpu-blocklist instead: a software Vulkan
# device is on Chromium's blocklist, and lavapipe needs no WSI workaround.
FLAGS="--headless=new --no-sandbox --disable-gpu-sandbox \
  $ANGLE_FLAGS --enable-features=Vulkan,UseSkiaRenderer \
  --enable-logging=stderr --allow-chrome-scheme-url \
  --virtual-time-budget=30000"

# chrome://gpu: informational only. Its feature-status table is rendered into a
# custom element's shadow DOM, which --dump-dom does not serialize, so this can
# never be scored as Vulkan evidence.
rm -rf /tmp/chromium-smoke-gpu
timeout "$CHROME_TIMEOUT" chromium $FLAGS --user-data-dir=/tmp/chromium-smoke-gpu \
  --dump-dom chrome://gpu > "$E/gpu-dom.html" 2> "$E/chromium-gpu.log"
gpu_rc=$?

# The WebGL probe is the real Vulkan/venus evidence: its post-script DOM holds
# the ANGLE unmasked renderer string. The <script> must live inside <body>; a
# head script runs before body exists and document.body is null.
cat > /workspace/smoke/webgl-probe.html <<'HTML'
<!doctype html>
<meta charset="utf-8">
<title>WebGL probe</title>
<body>
<script>
  var out = [];
  var c = document.createElement('canvas');
  var gl = c.getContext('webgl2') || c.getContext('webgl');
  if (!gl) {
    out.push('renderer=no-webgl');
  } else {
    var dbg = gl.getExtension('WEBGL_debug_renderer_info');
    out.push('renderer=' + (dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER)));
    out.push('vendor=' + (dbg ? gl.getParameter(dbg.UNMASKED_VENDOR_WEBGL) : ''));
    out.push('gl_version=' + gl.getParameter(gl.VERSION));
  }
  document.body.textContent = out.join('\n');
</script>
</body>
HTML

rm -rf /tmp/chromium-smoke-webgl
timeout "$CHROME_TIMEOUT" chromium $FLAGS --user-data-dir=/tmp/chromium-smoke-webgl \
  --window-size=900,600 --run-all-compositor-stages-before-draw \
  --screenshot="$E/webgl.png" "file:///workspace/smoke/webgl-probe.html" \
  > /dev/null 2> "$E/chromium-webgl.log"
webgl_rc=$?

rm -rf /tmp/chromium-smoke-dom
timeout "$CHROME_TIMEOUT" chromium $FLAGS --user-data-dir=/tmp/chromium-smoke-dom \
  --dump-dom "file:///workspace/smoke/webgl-probe.html" \
  > "$E/webgl-dom.html" 2> "$E/chromium-webgl-dom.log"
dom_rc=$?

# --dump-dom emits body text inline after the <body> tag, so the renderer line
# is not anchored at the start of a line; match it unanchored.
grep -oE 'renderer=[^<]*' "$E/webgl-dom.html" > "$E/webgl-renderer.txt" 2>/dev/null || true
grep -oE 'vendor=[^<]*' "$E/webgl-dom.html" >> "$E/webgl-renderer.txt" 2>/dev/null || true
grep -oE 'gl_version=[^<]*' "$E/webgl-dom.html" >> "$E/webgl-renderer.txt" 2>/dev/null || true

printf 'gpu-dom=%s webgl=%s dom=%s\n' "$gpu_rc" "$webgl_rc" "$dom_rc" > "$E/chromium-rc"

# Presenting-run outcome: mode, whether a waypipe display was even present, and
# whether the browser was still alive at the end of its dwell (if it died early,
# no host screenshot could have caught it).
if [ -n "$PRESENT_PID" ]; then
  present_alive=no
  kill -0 "$PRESENT_PID" 2>/dev/null && present_alive=yes
  {
    printf 'mode=%s wayland_display=%s gbm_backends_path=%s alive_after_dwell_secs=%s alive=%s\n' \
      "$MODE" "${WAYLAND_DISPLAY:-<unset>}" "${GBM_BACKENDS_PATH:-<unset>}" "$PRESENT_DWELL" "$present_alive"
    echo "--- presenting chromium log (gpu/error lines) ---"
    grep -aiE 'gpu process|not compatible|vulkan|render node|error|wayland' "$PRESENT_LOG" 2>/dev/null | head -10
  } > "$PRESENT_STATE" 2>&1
  kill "$PRESENT_PID" 2>/dev/null || true
fi

exit 0
