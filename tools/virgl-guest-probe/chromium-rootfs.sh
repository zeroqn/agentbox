#!/usr/bin/env bash
# Build a bare-libkrun VM rootfs variant that, on boot, runs chromium HEADLESS
# with ANGLE/Vulkan init against the venus virtio-gpu — reproducing the exact
# workload shape under which the in-product chromium GPU process hangs and is
# watchdog-killed (exit_code=6). The goal: isolate the chromium-init/ANGLE/WSI
# shape from the cang runtime context.
#
# Usage: bash chromium-rootfs.sh <probe-rootfs> <chromium-unwrapped-store-path>
# Output: printed store path of the variant rootfs (built under /tmp).
set -euo pipefail

BASE_ROOTFS="${1:?probe rootfs}"
CHROMIUM="${2:?chromium-unwrapped store path}"
OUT="${3:-/tmp/chromium-probe-rootfs}"

if [[ -e "$OUT" ]]; then
    chmod -R u+w "$OUT" 2>/dev/null || true
    rm -rf "$OUT" 2>/dev/null || true
fi
mkdir -p "$OUT"

# Nix-store files are r-xr-xr-x; make the tree writable so we can overwrite
# /init and add the chromium closure.
chmod -R u+w "$OUT" 2>/dev/null || true

# Copy the base probe rootfs (it already has the store closure + init for the
# venus probe). Then add the chromium closure on top.
cp -a "$BASE_ROOTFS"/. "$OUT"/
# The base's /nix/store is read-only from the cp; make it writable so the
# chromium closure copies (below) can land.
chmod -R u+w "$OUT/nix/store" 2>/dev/null || true

echo "assembling chromium probe rootfs at $OUT"
echo "chromium store: $CHROMIUM"

# Copy the chromium-unwrapped closure into $OUT/nix/store (absolute store paths
# must resolve identically inside the VM).  Only paths not already present are
# copied.  set +e: a non-zero from the producer (nix-store -qR closes the pipe)
# must NOT abort the loop.
mkdir -p "$OUT/nix/store"
set +e
while read -r p; do
    [[ -z "$p" ]] && continue
    base="$(basename "$p")"
    if [[ ! -e "$OUT/nix/store/$base" ]]; then
        cp -a "$p" "$OUT/nix/store/" 2>/dev/null
    fi
done < <(nix-store -qR "$CHROMIUM")
set -e

# The wrapper's LD_LIBRARY_PATH/XDG_DATA_DIRS reference additional store paths
# (gtk3/gtk4/wayland/krb5/libva/pipewire and share dirs) that the wrapper
# script hard-codes; copy those too via the wrapper's own closure.
WRAPPER=/nix/store/53p8msmqxpi829zdrw6qkvaamidxy9cj-chromium-151.0.7922.173
set +e
while read -r p; do
    base="$(basename "$p")"
    [[ -e "$OUT/nix/store/$base" ]] && continue
    cp -a "$p" "$OUT/nix/store/" 2>/dev/null
done < <(nix-store -qR "$WRAPPER")
set -e

# Ensure a sh exists for any shebang.
if [[ ! -x "$OUT/bin/sh" ]]; then
    ln -sf "${BASE_ROOTFS}/bin/busybox" "$OUT/bin/busybox" 2>/dev/null || true
    ln -sf busybox "$OUT/bin/sh" 2>/dev/null || true
fi

echo "chromium closure paths: $(nix-store -qR "$CHROMIUM" | wc -l)"
du -sh "$OUT"

# Overwrite /init to run chromium headless with the exact live-smoke ANGLE/Vulkan
# flags, capturing GPU-process result + chrome://gpu + WebGL evidence.
chmod -R u+w "$OUT" 2>/dev/null || true
CHROME_BIN="$CHROMIUM/libexec/chromium/chromium"
# The base probe rootfs's init already defines the exact guest store paths.
MESA_ICD="/nix/store/6q9zxz6km0z4dmlxi6yrdp8rccbh49m1-mesa-26.1.8/share/vulkan/icd.d/virtio_icd.x86_64.json"
VK_LOAD_LIB="/nix/store/9n05z8yjq7ji5w8cj9mk0frrf7xc0jgq-vulkan-loader-1.4.341.0/lib"
MESA_LIB="/nix/store/6q9zxz6km0z4dmlxi6yrdp8rccbh49m1-mesa-26.1.8/lib"

cat > "$OUT/init" <<EOF
#!/bin/sh
BB="/nix/store/d5x4mlpb0zzwmgzjzwljb0vaifvfy7r9-busybox-1.37.0/bin/busybox"
"\$BB" mount -t proc proc /proc 2>/dev/null
"\$BB" mount -t sysfs sysfs /sys 2>/dev/null
"\$BB" mount -t devtmpfs devtmpfs /dev 2>/dev/null
"\$BB" mkdir -p /dev/dri /tmp /run
export VK_DRIVER_FILES="$MESA_ICD"
export LD_LIBRARY_PATH="$VK_LOAD_LIB:$MESA_LIB"
export XDG_RUNTIME_DIR=/tmp
export CHROME_DEVEL_SANDBOX=/nonexistent
echo "[init] chromium headless venus test start"
cd /tmp
"\$BB" timeout 150 "$CHROME_BIN" --headless=new --no-sandbox --disable-gpu-sandbox \
  --use-angle=vulkan --enable-features=Vulkan,UseSkiaRenderer \
  --disable-features=VulkanFromANGLE \
  --enable-logging=stderr --virtual-time-budget=25000 \
  --dump-dom "data:text/html,<title>GPU probe</title><body id=gpu>ok</body>" \
  > /chromium-headless.log 2>&1
rc=\$?
echo "[init] chromium exit=\$rc"
echo "[init] === chromium-headless.log (head) ==="
"\$BB" head -80 /chromium-headless.log 2>/dev/null || true
echo "[init] chromium headless test done"
EOF
chmod +x "$OUT/init"

echo "$OUT"