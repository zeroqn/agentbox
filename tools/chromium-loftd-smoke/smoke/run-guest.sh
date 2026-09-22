#!/bin/sh
# Runs inside the loftd microVM. The nixpkgs `ungoogled-chromium` wrapper from
# the loftd image (browserImageLayer) provides LD_LIBRARY_PATH/XDG_DATA_DIRS
# itself, so there is no environment setup here. Evidence is written to
# /workspace/evidence (a host-visible bind) so the host runner can score it.
#
# Guest always exits 0; Chromium's own result is judged from the evidence
# files, keeping the VM exit code and the browser exit code distinct.
set -u
E=/workspace/evidence
mkdir -p "$E"

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
  env | grep -iE 'VK_|LOFTD_GPU|LIBGL|MESA|EGL|GALLIUM|DRM' 2>&1
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

# Every Chromium invocation is bounded: the venus/render-server path being
# exercised is known to stall, and a stall must be attributed to one run
# instead of hanging until the host-side VM timeout kills everything.
CHROME_TIMEOUT=150

: > "$E/version.txt"
chromium --version > "$E/version.txt" 2>&1 || true

FLAGS="--headless=new --no-sandbox --disable-gpu-sandbox \
  --use-angle=vulkan --enable-features=Vulkan,UseSkiaRenderer \
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

exit 0
