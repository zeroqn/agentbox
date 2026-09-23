#!/usr/bin/env bash
# External render-server venus probe: boot a bare libkrun VM with the EXACT
# product render-server path (options3 + SOCK_SEQPACKET fd + sandboxed
# external virgl_render_server) and check the guest's venus submit fence.
#
# Usage:
#   bash run-rs.sh
#
# Builds the same guest rootfs + guest-probe as run.sh, but launches through
# launcher-rs.c which mirrors cang's render_server.rs: socketpair -> spawn
# virgl_render_server --socket-fd=<child> -> krun_set_gpu_options3(ctx, 0xe43,
# shm, parent_fd). Requires the dev-shell env (RENDER_SERVER_EXEC_PATH and an
# LD_LIBRARY_PATH that resolves libkrun.so + libvirglrenderer.so.1 + the
# host-side 64-bit vulkan loader).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOTFS="$(nix build "$HERE#guest-rootfs" --no-link --print-out-paths)"
echo "rootfs: $ROOTFS"

VKLOADER="$(ls -d "$ROOTFS"/nix/store/*-vulkan-loader-*/lib 2>/dev/null | head -1)"
MESA="$(ls -d "$ROOTFS"/nix/store/*-mesa-*/lib 2>/dev/null | head -1)"
if [[ -z "$VKLOADER" || -z "$MESA" ]]; then
    echo "error: cannot resolve host vulkan-loader or mesa from rootfs" >&2
    exit 1
fi
MESAICD="$(ls "$ROOTFS"/nix/store/*-mesa-*/share/vulkan/icd.d/radeon_icd.x86_64.json 2>/dev/null | head -1)"
if [[ -z "$MESAICD" ]]; then
    echo "note: no radeon ICD on this host; falling back to lavapipe (software)" >&2
    MESAICD="$(ls "$ROOTFS"/nix/store/*-mesa-*/share/vulkan/icd.d/lvp_icd.x86_64.json 2>/dev/null | head -1)"
fi
if [[ -z "$MESAICD" ]]; then
    echo "error: cannot resolve a host ICD from rootfs" >&2
    exit 1
fi
echo "VK_DRIVER_FILES=$MESAICD"

echo "=== building launcher-rs (nix build .#launcher-rs) ==="
LAUNCHER="$(nix build "$HERE#launcher-rs" --no-link --print-out-paths)"
echo "launcher-rs: $LAUNCHER"

if [[ -z "${RENDER_SERVER_EXEC_PATH:-}" ]]; then
    vgl="$(ls -d /nix/store/*-virglrenderer-1.3.0/libexec/virgl_render_server 2>/dev/null | head -1)"
    vgl="${vgl:-/nix/store/0gsa8cpgc8ai4gcj3gm3j7mwv0wpq7q9-virglrenderer-1.3.0/libexec/virgl_render_server}"
    export RENDER_SERVER_EXEC_PATH="$vgl"
fi
echo "RENDER_SERVER_EXEC_PATH=$RENDER_SERVER_EXEC_PATH"
export LD_LIBRARY_PATH="${VKLOADER}:${MESA}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export VK_DRIVER_FILES="$MESAICD"

BASE="$(mktemp -d)"
CONSOLE="$BASE/guest-console.log"
trap 'rm -rf "$BASE"' EXIT

echo "=== booting libkrun VM via options3 + external render server (flags=0xe43) ==="
set +e
"$LAUNCHER/bin/launcher-rs" "$ROOTFS" "$CONSOLE"
rc=$?
set -e
echo "=== launcher-rs exit=$rc ==="
echo
echo "=== guest console session ==="
cat "$CONSOLE" 2>/dev/null || true

echo "=== verdict ==="
if grep -q "RESULT: PASS" "$CONSOLE" 2>/dev/null; then
    echo "PASS: external render-server path (options3 + SOCK_SEQPACKET) completes a venus submit fence."
    exit 0
elif grep -q "RESULT: FAIL\|FAIL" "$CONSOLE" 2>/dev/null; then
    echo "FAIL: the external render-server venus path did not produce a usable device or completed fence."
    exit 1
else
    echo "INCONCLUSIVE: no probe verdict in the console (see log above)."
    exit 2
fi