#!/bin/sh
# Runs inside the loftd microVM. The nixpkgs `chromium` wrapper from the loftd
# image (browserImageLayer) provides LD_LIBRARY_PATH/XDG_DATA_DIRS itself, so
# there is no environment setup here. Evidence is written to /workspace/evidence
# (a host-visible bind) so the host runner can score it.
#
# Guest always exits 0; Chromium's own result is judged from the evidence
# files, keeping the VM exit code and the browser exit code distinct.
set -u
E=/workspace/evidence
mkdir -p "$E"

: > "$E/version.txt"
chromium --version > "$E/version.txt" 2>&1 || true

FLAGS="--headless=new --no-sandbox --disable-gpu-sandbox \
  --use-angle=vulkan --enable-features=Vulkan,UseSkiaRenderer \
  --disable-features=VulkanFromANGLE --enable-logging=stderr \
  --allow-chrome-scheme-url --user-data-dir=/tmp/chromium-smoke \
  --virtual-time-budget=30000"

: > "$E/chromium-rc"
: > "$E/gpu-dom.html"
chromium $FLAGS --dump-dom chrome://gpu > "$E/gpu-dom.html" 2> "$E/chromium-gpu.log"
echo "gpu-dom rc=$?" >> "$E/chromium-rc"

cat > /workspace/smoke/webgl-probe.html <<'HTML'
<!doctype html>
<meta charset="utf-8">
<title>WebGL probe</title>
<script>
  var c = document.createElement('canvas');
  var gl = c.getContext('webgl2') || c.getContext('webgl');
  if (!gl) {
    document.body.textContent = 'no-webgl';
  } else {
    var dbg = gl.getExtension('WEBGL_debug_renderer_info');
    var r = dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
    var v = dbg ? gl.getParameter(dbg.UNMASKED_VENDOR_WEBGL) : '';
    document.body.textContent = 'renderer=' + r + '\nvendor=' + v;
  }
</script>
HTML

: > "$E/webgl-renderer.txt"
chromium $FLAGS --window-size=900,600 --run-all-compositor-stages-before-draw \
  --screenshot="$E/webgl.png" --file:///workspace/smoke/webgl-probe.html \
  > /dev/null 2> "$E/chromium-webgl.log"
echo "webgl rc=$?" >> "$E/chromium-rc"

# Capture the probe page's post-script DOM (document.body.textContent now holds
# the renderer string) and extract it for the host scorer.
: > "$E/webgl-dom.html"
chromium $FLAGS --dump-dom --file:///workspace/smoke/webgl-probe.html \
  > "$E/webgl-dom.html" 2> "$E/chromium-webgl-dom.log"
grep -E '^renderer=' "$E/webgl-dom.html" > "$E/webgl-renderer.txt" 2>/dev/null || true

exit 0