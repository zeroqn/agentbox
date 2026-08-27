#!/usr/bin/env bash
# Build the guest rootfs + launcher, boot the libkrun VM, and report whether the
# guest saw a venus Vulkan device.
#
# Usage:
#   bash run.sh [gpu-flags-hex]
#   nix run .                       # equivalent (uses the flake's devShell env)
#
# The launcher routes the guest console to a host file; this script greps it for
# the probe verdict.  RENDER_SERVER_EXEC_PATH must point at the render server of
# the same virglrenderer the libkrun closure links (its libvirglrenderer.so.1).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FLAGS="${1:-0x2c0}"

# Resolve the virglrenderer that libkrun actually links (its resolved lib path)
# so RENDER_SERVER_EXEC_PATH matches the libvirglrenderer loaded at runtime.
# Uses `ldd` (not `readelf`) because readelf is not part of the default NixOS
# user environment.
find_libkrun_vgl() {
    local lib
    for lib in /nix/store/*-libkrun-loftd-*/lib/libkrun.so; do
        local rp
        rp="$(ldd "$lib" 2>/dev/null | grep -oE '/nix/store/[A-Za-z0-9.+-]+-virglrenderer-1\.3\.0/lib' | head -1)"
        if [[ -n "$rp" ]]; then
            echo "${rp%/lib}"
            return 0
        fi
    done
    return 1
}

# Preferred: the virglrenderer package from the nixpkgs the launcher was built
# with (the flake devShell sets RENDER_SERVER_EXEC_PATH already).  Fall back to
# discovering it from a libkrun store path.
if [[ -z "${RENDER_SERVER_EXEC_PATH:-}" ]]; then
    vgl="$(find_libkrun_vgl || true)"
    if [[ -n "$vgl" ]]; then
        export RENDER_SERVER_EXEC_PATH="$vgl/libexec/virgl_render_server"
    else
        echo "error: cannot find virgl_render_server; set RENDER_SERVER_EXEC_PATH" >&2
        exit 1
    fi
fi
echo "RENDER_SERVER_EXEC_PATH=$RENDER_SERVER_EXEC_PATH"

echo "=== building guest rootfs (nix build .#guest-rootfs) ==="
ROOTFS="$(nix build .#guest-rootfs --no-link --print-out-paths)"
echo "rootfs: $ROOTFS"

# Resolve the host-side 64-bit vulkan-loader and mesa from the rootfs closure
# (which uses the same flake nixpkgs).  The render server's venus renderer (vkr)
# dlopens libvulkan.so.1 on the host; it needs a 64-bit loader and a host ICD.
# Lavapipe (lvp) is a pure-software Vulkan ICD that works headless — ideal for
# this diagnostic probe.
VKLOADER="$(ls -d "$ROOTFS"/nix/store/*-vulkan-loader-*/lib 2>/dev/null | head -1)"
MESA="$(ls -d "$ROOTFS"/nix/store/*-mesa-*/lib 2>/dev/null | head -1)"
MESAICD="$(ls "$ROOTFS"/nix/store/*-mesa-*/share/vulkan/icd.d/lvp_icd.x86_64.json 2>/dev/null | head -1)"
if [[ -z "$VKLOADER" || -z "$MESA" || -z "$MESAICD" ]]; then
    echo "error: cannot resolve host vulkan-loader or mesa from rootfs" >&2
    exit 1
fi
export LD_LIBRARY_PATH="${VKLOADER}:${MESA}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export VK_DRIVER_FILES="$MESAICD"
echo "VK_DRIVER_FILES=$VK_DRIVER_FILES"

echo "=== building launcher (nix build .#launcher) ==="
LAUNCHER="$(nix build .#launcher --no-link --print-out-paths)"
echo "launcher: $LAUNCHER"

BASE="$(mktemp -d)"
CONSOLE="$BASE/guest-console.log"
trap 'rm -rf "$BASE"' EXIT

echo "=== booting libkrun VM (gpu_flags=$FLAGS) ==="
set +e
RENDER_SERVER_EXEC_PATH="$RENDER_SERVER_EXEC_PATH" \
    "$LAUNCHER/bin/launcher" "$ROOTFS" "$CONSOLE" "$FLAGS"
rc=$?
set -e
echo "=== launcher exit=$rc (kernel/workload may have produced output above) ==="

echo
echo "=== guest console session ==="
cat "$CONSOLE" 2>/dev/null || true

echo
echo "=== verdict ==="
if grep -q "RESULT: PASS" "$CONSOLE" 2>/dev/null; then
    echo "PASS: the libkrun guest exposed a venus Vulkan device and created a logical device."
    exit 0
elif grep -q "RESULT: FAIL\|FAIL" "$CONSOLE" 2>/dev/null; then
    echo "FAIL: the guest did not get a usable venus Vulkan device (see console above)."
    exit 1
else
    echo "INCONCLUSIVE: no probe verdict in the console (see log above)."
    exit 2
fi