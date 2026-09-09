#!/usr/bin/env bash
# Reproducible Chromium GPU live smoke for loftd.
#
# Every input is pinned in the repository: the loftd image (with the
# browserImageLayer carrying nixpkgs#ungoogled-chromium), the packaged loftd
# binary, the guest-init override, and an isolated config/state home. The
# browser is NOT mounted from the host; it is part of the image, so resource
# files mmap from the digest-keyed image rootfs (host /nix overlay lowerdir)
# instead of the fuse upper.
#
# Output: <out>/evidence/*  (fresh, host-visible bind /workspace/evidence)
#         <out>/logs/*      (loftd.console, guest chromium logs)
# Exit 0 only when fresh, non-empty evidence proves: chromium version,
# chrome://gpu DOM with ANGLE/Vulkan status, a non-SwiftShader WebGL renderer,
# a GPU-composited PNG, and chromium exit rc 0 for both runs.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

loftd_bin="${LOFTD_BIN:-}"
guest_init="${LOFTD_GUEST_INIT:-}"
container_ref=""
state_home=""
mem_gib=4
timeout_seconds=600
keep_evidence=0
out_dir=""
no_default_flag_set=0

usage() {
  cat <<'USAGE'
Usage: chromium-smoke.sh [OPTIONS]

Run a reproducible Chromium GPU live smoke inside a real loftd microVM. The
browser travels inside the loftd image (browserImageLayer); nothing is mounted
from the host. Evidence is written to a guest-visible host bind and checked
for freshness (mtime >= run start) so stale artifacts can never pass.

Options:
      --loftd <path>         loftd binary (default: $LOFTD_BIN or nix build .#loftd)
      --guest-init <path>    guest-init override (default: $LOFTD_GUEST_INIT, else nix build .#agentbox-musl if its bin/loftd-guest-init exists)
      --container <ref|path> image for this run (default: nix build .#container; a
                             store path is loaded product-style into the local store)
      --state-home <path>    XDG_STATE_HOME (default: <out>/state)
      --mem <GiB>            loftd --mem (default: 4)
      --timeout <seconds>    live vm timeout (default: 600)
      --keep-evidence        do not delete the previous evidence dir
      --out-dir <path>       output root (default: .smoke/chromium-loftd/<timestamp>)
      --no-default-flags     do not add the verified --gpu=drm --alloc hardened
                             --seccomp=off --landlock=off default flags
  -h, --help                 show this help

The verified working launch shape (used by the chromium GPU investigation) is:
  loftd --gpu=drm --alloc hardened --mem <n> --seccomp=off --landlock=off
The smoke keeps the --gpu=drm --alloc hardened pair (hardened_malloc is
required; mimalloc is incompatible with Chromium partition_alloc) and the
--seccomp=off --landlock=off pair so the guest GPU path is exercised without
host-seccomp SIGSYS interference. Re-enabling host policy is a separate
hardening task, not part of reproducing the GPU evidence.
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --loftd) loftd_bin="${2:?missing value for --loftd}"; shift 2 ;;
    --guest-init) guest_init="${2:?missing value for --guest-init}"; shift 2 ;;
    --container) container_ref="${2:?missing value for --container}"; shift 2 ;;
    --state-home) state_home="${2:?missing value for --state-home}"; shift 2 ;;
    --mem) mem_gib="${2:?missing value for --mem}"; shift 2 ;;
    --timeout) timeout_seconds="${2:?missing value for --timeout}"; shift 2 ;;
    --keep-evidence) keep_evidence=1; shift ;;
    --out-dir) out_dir="${2:?missing value for --out-dir}"; shift 2 ;;
    --no-default-flags) no_default_flag_set=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 1 ;;
  esac
done

case "$mem_gib" in ''|*[!0-9]*) echo "--mem must be a positive integer" >&2; exit 1 ;; esac
[ "$mem_gib" -ge 1 ] || { echo "--mem must be >= 1" >&2; exit 1; }
case "$timeout_seconds" in ''|*[!0-9]*) echo "--timeout must be a positive integer" >&2; exit 1 ;; esac

[ -x "$loftd_bin" ] || loftd_bin="$(nix build "$repo_root#loftd" --print-out-paths 2>/dev/null || true)"
loftd_bin="${loftd_bin%/bin/loftd}/bin/loftd"
[ -x "$loftd_bin" ] || { echo "loftd binary not resolved or not executable: $loftd_bin" >&2; exit 2; }

if [ -n "$guest_init" ]; then
  :
elif [ -x "$repo_root/result/bin/loftd-guest-init" ]; then
  guest_init="$repo_root/result/bin/loftd-guest-init"
else
  musl="$(nix build "$repo_root#agentbox-musl" --print-out-paths 2>/dev/null || true)"
  if [ -n "$musl" ] && [ -x "$musl/bin/loftd-guest-init" ]; then
    guest_init="$musl/bin/loftd-guest-init"
  fi
fi
if [ -n "$guest_init" ]; then
  [ -x "$guest_init" ] || { echo "guest-init not executable: $guest_init" >&2; exit 2; }
fi

if [ -z "$out_dir" ]; then
  out_dir="$repo_root/.smoke/chromium-loftd/$(date -u +%Y%m%dT%H%M%SZ)"
fi
mkdir -p "$out_dir/logs" "$out_dir/config/loftd"
[ -n "$state_home" ] || state_home="$out_dir/state"

# Image: build the flake container (which runs image wrapper checks) and load
# the OCI archive into a hermetic storage, never the host's ambient
# ~/.config/containers/storage.conf (which may point at a broken btrfs path).
if [ -z "$container_ref" ]; then
  container_path="$(nix build "$repo_root#container" --print-out-paths)"
  container_ref="$container_path"
fi
if [[ "$container_ref" == /* ]]; then
  # Hermetic podman/buildah storage: vfs driver (no mount deps), fresh per run,
  # on the same disk as the archive (the /nix store copy). TMPDIR stays local.
  container_storage_dir="$out_dir/container-storage"
  mkdir -p "$container_storage_dir/graph" "$container_storage_dir/run" "$container_storage_dir/tmp"
  cat > "$container_storage_dir/storage.conf" <<EOF
[storage]
driver = "vfs"
graphroot = "$container_storage_dir/graph"
runroot = "$container_storage_dir/run"
EOF
  container_fs_free_kib=$(df -Pk "$container_storage_dir" | awk 'NR==2 {print $4}')
  container_size_kib=$(du -sk "$container_ref" 2>/dev/null | awk '{print $1}')
  needed_kib=$((20 * 1024 * 1024))
  if [ "${container_fs_free_kib:-0}" -lt "$needed_kib" ]; then
    echo "FATAL: not enough free space on $container_storage_dir (free ${container_fs_free_kib} KiB, need ${needed_kib} KiB for archive ${container_size_kib} KiB). Free disk space first." >&2
    exit 2
  fi
  export CONTAINERS_STORAGE_CONF="$container_storage_dir/storage.conf"
  export TMPDIR="$container_storage_dir/tmp"
  echo "loading image archive $container_ref into hermetic storage ($container_storage_dir)"
  podman load -i "$container_ref" >/dev/null
  container_ref="localhost/loftd:latest"
fi

# Backend: deterministic choice, not environment luck. btrfs-snapshot only when
# the state-home path itself is on a btrfs filesystem; otherwise fuse-overlay.
backend="fuse-overlay"
if findmnt -t btrfs --target "$state_home" >/dev/null 2>&1; then
  backend="btrfs-snapshot"
fi

# Isolated loftd config: hermetic state location + explicit backend. Never
# depends on the user's real ~/.config/loftd/loftd.toml.
cat > "$out_dir/config/loftd/loftd.toml" <<EOF
[state]
location = "$state_home"

[task-rootfs]
backend = "$backend"
EOF

# Workspace = guest /workspace. Fresh evidence only (never reuse across runs).
workspace="$out_dir/workspace"
mkdir -p "$workspace/smoke"
cp "$tool_dir/smoke/run-guest.sh" "$workspace/smoke/run-guest.sh"
chmod +x "$workspace/smoke/run-guest.sh"
if [ "$keep_evidence" -eq 0 ]; then
  rm -rf "$workspace/evidence"
fi
mkdir -p "$workspace/evidence"

launch_s="$(date +%s)"

loftd_args=(--mem "$mem_gib" --gpu=drm --alloc hardened --seccomp=off --landlock=off)
if [ "$no_default_flag_set" -eq 1 ]; then
  loftd_args=(--mem "$mem_gib")
fi
if [ -n "$guest_init" ]; then
  loftd_args+=(--guest-init "$guest_init")
fi

echo "loftd: $loftd_bin"
echo "image: $container_ref"
echo "backend: $backend | state-home: $state_home"
echo "launch command: $loftd_bin ${loftd_args[*]} -- sh /workspace/smoke/run-guest.sh"

( cd "$workspace" && env XDG_CONFIG_HOME="$out_dir/config" XDG_STATE_HOME="$state_home" \
    LOFTD_IMAGE="$container_ref" \
    timeout "$timeout_seconds" \
    script -q -e -c "$loftd_bin ${loftd_args[*]} -- sh /workspace/smoke/run-guest.sh" /dev/null ) \
  >"$out_dir/logs/loftd.console" 2>&1 && vm_rc=0 || vm_rc=$?

if [ "$vm_rc" -eq 124 ]; then
  echo "FATAL: loftd timed out after ${timeout_seconds}s (log: $out_dir/logs/loftd.console)" >&2
  exit 124
fi
echo "loftd vm exit: $vm_rc (see $out_dir/logs/loftd.console)"

fail=0
E="$workspace/evidence"
fresh() { f="$1"; [ -s "$f" ] && [ "$(stat -c %Y "$f")" -ge "$launch_s" ]; }
check() { # check <name> <path> <desc> <predicate-args...>
  local name="$1" path="$2" desc="$3"; shift 3
  if fresh "$path" && "$@" < "$path"; then
    echo "PASS  $name ($desc)"
  else
    echo "FAIL  $name ($desc) -> $path missing/stale/empty or predicate failed"
    fail=1
  fi
}

check version "version.txt" "Chromium version" \
  grep -Eq 'Chromium [0-9]'
check gpu-dom "gpu-dom.html" "chrome://gpu DOM with ANGLE/Vulkan status" \
  grep -Eq 'Vulkan|ANGLE|feature status|GPU feature'
check webgl-renderer "webgl-renderer.txt" "non-SwiftShader WebGL renderer" \
  grep -Eqv 'SwiftShader' <(grep -E 'renderer=' "$E/webgl-renderer.txt" 2>/dev/null)
check chromium-rc "chromium-rc" "both chromium runs exit 0" \
  grep -Eq 'gpu-dom rc=0[[:space:]]*webgl rc=0' "$E/chromium-rc"

# PNG: non-empty + magic bytes
if fresh "$E/webgl.png" && [ "$(head -c 4 "$E/webgl.png" | od -An -tx1 | tr -d ' \n')" = "89504e47" ]; then
  echo "PASS  webgl-png (non-empty PNG screenshot)"
else
  echo "FAIL  webgl-png -> $E/webgl.png missing/stale/empty or not a PNG"
  fail=1
fi

if [ "$fail" -eq 0 ]; then
  echo "VERDICT: PASS — evidence: $E (fresh, $(stat -c %Y "$E"/* 2>/dev/null | wc -l) files)"
  exit 0
fi

echo "VERDICT: FAIL — inspect:"
echo "  evidence: $E"
echo "  console:  $out_dir/logs/loftd.console"
echo "  guest chromium logs are mirrored into the evidence dir (chromium-gpu.log, chromium-webgl.log)"
exit 1