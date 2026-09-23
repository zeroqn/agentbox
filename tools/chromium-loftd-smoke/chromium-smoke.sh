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
# Output: <out>/workspace/evidence/*  (fresh, host-visible bind /workspace/evidence)
#         <out>/logs/*                (loftd.console)
# With --waypipe it additionally boots a host Wayland compositor (weston,
# headless + GL) and a host waypipe client, launches loftd with
# --waypipe=<socket>, and scores the waypipe transport: the guest's waypipe
# server connecting, the guest Chromium rendering on venus while presenting
# through waypipe, the page's frame arriving in the compositor (its magenta
# background and the cyan renderer overlay the page prints), and a
# no---waypipe control run that must NOT deliver that frame. The frame and its
# control are also copied out as weston-screenshot*.png for a human to open.
#
# Exit 0 only when fresh, non-empty evidence proves: chromium version, an
# ANGLE WebGL renderer on the Vulkan backend (not SwiftShader) from the probe
# page, a GPU-composited PNG, and rc 0 for every chromium run. The chrome://gpu
# dump is captured for humans but unscored: its feature table lives in shadow
# DOM, which --dump-dom does not serialize.
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
waypipe_mode=0
weston_bin="${WESTON_BIN:-}"
weston_renderer="gl"
present_wait=30
waypipe_bin="${WAYPIPE_BIN:-}"
python_bin="${PYTHON:-}"

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
      --waypipe              also run and score the waypipe presenting run (host
                             weston + waypipe client; needs --weston/--waypipe-bin
                             and python3 - see the --waypipe mode section below)
      --weston <path>        weston binary (default: $WESTON_BIN, else PATH)
      --waypipe-bin <path>   waypipe binary (default: $WAYPIPE_BIN, else PATH)
      --weston-renderer <r>  compositor renderer: gl (default) or pixman
      --present-wait <secs>  seconds after launch before the first compositor
                             screenshot (default: 30)
      --python <path>        python3 used to read the host screenshot pixels
                             (default: $PYTHON, else PATH)
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
    --waypipe) waypipe_mode=1; shift ;;
    --weston) weston_bin="${2:?missing value for --weston}"; shift 2 ;;
    --weston-renderer) weston_renderer="${2:?missing value for --weston-renderer}"; shift 2 ;;
    --present-wait) present_wait="${2:?missing value for --present-wait}"; shift 2 ;;
    --waypipe-bin) waypipe_bin="${2:?missing value for --waypipe-bin}"; shift 2 ;;
    --python) python_bin="${2:?missing value for --python}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 1 ;;
  esac
done

case "$mem_gib" in ''|*[!0-9]*) echo "--mem must be a positive integer" >&2; exit 1 ;; esac
[ "$mem_gib" -ge 1 ] || { echo "--mem must be >= 1" >&2; exit 1; }
case "$timeout_seconds" in ''|*[!0-9]*) echo "--timeout must be a positive integer" >&2; exit 1 ;; esac
case "$present_wait" in ''|*[!0-9]*) echo "--present-wait must be a positive integer" >&2; exit 1 ;; esac
case "$weston_renderer" in gl|pixman) : ;; *) echo "--weston-renderer must be gl or pixman" >&2; exit 1 ;; esac
[ -n "$weston_bin" ] || weston_bin="$(command -v weston || true)"
[ -n "$waypipe_bin" ] || waypipe_bin="$(command -v waypipe || true)"
[ -n "$python_bin" ] || python_bin="$(command -v python3 || true)"

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
# The top-level screenshots are this run's copies of scored frames. A reused
# --out-dir would otherwise leave the previous run's image behind, and the
# verdict would point a human at a frame from an older run.
rm -f "$out_dir/weston-screenshot.png" "$out_dir/weston-screenshot-control.png"
[ -n "$state_home" ] || state_home="$out_dir/state"

# loftd's only implemented task-rootfs backend is btrfs-snapshot, and it
# snapshots the Buildah-mounted rootfs, so BOTH the hermetic container-store
# graphroot and the loftd state home must live on btrfs. Create the
# directories first: `findmnt --target` fails on a path that does not exist
# yet, which silently downgraded earlier runs to the unimplemented
# fuse-overlay backend and a non-snapshottable vfs graphroot.
mkdir -p "$state_home" "$out_dir/container-storage/graph"
for required_btrfs_dir in "$out_dir/container-storage/graph" "$state_home"; do
  if ! findmnt -t btrfs --target "$required_btrfs_dir" >/dev/null 2>&1; then
    echo "FATAL: $required_btrfs_dir must be on btrfs; the btrfs-snapshot task-rootfs backend snapshots the Buildah graphroot, and neither the fuse-overlay backend nor a vfs graphroot can be snapshotted" >&2
    echo "       put --out-dir (and --state-home) on a btrfs filesystem." >&2
    exit 2
  fi
done
backend="btrfs-snapshot"

# Image: build the flake container (which runs image wrapper checks) and load
# the OCI archive into a hermetic storage, never the host's ambient
# ~/.config/containers/storage.conf (which may point at a broken btrfs path).
if [ -z "$container_ref" ]; then
  container_path="$(nix build "$repo_root#container" --print-out-paths)"
  container_ref="$container_path"
fi
if [[ "$container_ref" == /* ]]; then
  # Hermetic podman/buildah storage using the btrfs driver: it creates one
  # subvolume per layer, which is exactly what btrfs-snapshot needs. A vfs
  # graphroot is plain directories and cannot be snapshotted. Fresh per run;
  # the ambient ~/.config/containers/storage.conf is never used.
  container_storage_dir="$out_dir/container-storage"
  mkdir -p "$container_storage_dir/graph" "$container_storage_dir/run" "$container_storage_dir/tmp"
  cat > "$container_storage_dir/storage.conf" <<EOF
[storage]
driver = "btrfs"
graphroot = "$container_storage_dir/graph"
runroot = "$container_storage_dir/run"
EOF
  container_fs_free_kib=$(df -Pk "$container_storage_dir" | awk 'NR==2 {print $4}')
  container_size_kib=$(du -sk "$container_ref" 2>/dev/null | awk '{print $1}')
  # The unpacked image is ~3x the compressed archive; 12 GiB covers the
  # graphroot plus the btrfs-snapshot task state (which is CoW-shared).
  needed_kib=$((12 * 1024 * 1024))
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
# The presenting run's pattern page travels with the workspace: the guest has no
# access to the repo, so anything the guest executes or loads is staged here.
cp "$tool_dir/smoke/waypipe-present.html" "$workspace/smoke/waypipe-present.html"
if [ "$keep_evidence" -eq 0 ]; then
  rm -rf "$workspace/evidence"
fi
mkdir -p "$workspace/evidence"

# ---- waypipe presenting run: compositor + client ---------------------------
# Both must exist BEFORE loftd starts: loftd preflights the socket
# ("waypipe socket does not exist" / "waypipe transport is not a Unix socket"),
# and the guest's waypipe server dials the client lazily, on the guest app's
# first connection.
weston_pid=""
waypipe_client_pid=""
weston_dir=""
cleanup() {
  [ -n "$waypipe_client_pid" ] && kill "$waypipe_client_pid" 2>/dev/null || true
  [ -n "$weston_pid" ] && kill "$weston_pid" 2>/dev/null || true
  return 0
}
trap cleanup EXIT

host_shot() { # host_shot <target-png>   capture the compositor output
  local target="$1" shot
  ( cd "$waypipe_shot" || exit 1
    env XDG_RUNTIME_DIR="$waypipe_run" WAYLAND_DISPLAY="$weston_socket" \
      timeout 30 "$weston_dir/weston-screenshooter" >>"$out_dir/logs/screenshooter.log" 2>&1
    for shot in wayland-screenshot-*.png; do
      [ -f "$shot" ] || continue
      mv "$shot" "$target"
      break
    done ) >/dev/null 2>&1 || true
}

start_watcher() { # start_watcher <early-png> <late-png>
  # The guest dwells on the presenting run for its own window (see
  # run-guest.sh PRESENT_DWELL), so the two captures sit inside it.
  ( sleep "$present_wait"; host_shot "$1"; sleep 30; host_shot "$2" ) &
  watcher_pid=$!
}

if [ "$waypipe_mode" -eq 1 ]; then
  [ -x "$weston_bin" ] || {
    echo "FATAL: --waypipe needs weston; build it with 'nix build nixpkgs#weston' and pass --weston <path>" >&2
    exit 2
  }
  [ -x "$waypipe_bin" ] || {
    echo "FATAL: --waypipe needs waypipe; build it with 'nix build nixpkgs#waypipe' and pass --waypipe-bin <path>" >&2
    exit 2
  }
  [ -n "$python_bin" ] && [ -x "$python_bin" ] || {
    echo "FATAL: --waypipe needs python3 to read the host screenshot pixels (run under 'nix develop', or pass --python <path>)" >&2
    exit 2
  }
  waypipe_run="$out_dir/waypipe/run"
  waypipe_shot="$out_dir/waypipe/shot"
  weston_socket="loftd-smoke"
  mkdir -p "$waypipe_run" "$waypipe_shot"
  chmod 700 "$waypipe_run"
  weston_dir="$(dirname "$weston_bin")"
  waypipe_sock="$out_dir/waypipe/waypipe.sock"

  # A reused --out-dir leaves the previous run's sockets behind. A leftover
  # socket file makes the new listener fail with EADDRINUSE *and* satisfies the
  # "-S" readiness test, so it has to go before anything is started.
  rm -f "$waypipe_run/$weston_socket" "$waypipe_sock"
  echo "compositor: $weston_bin (backend=headless renderer=$weston_renderer socket=$weston_socket)"
  env XDG_RUNTIME_DIR="$waypipe_run" "$weston_bin" --backend=headless \
    --renderer="$weston_renderer" --debug --width=640 --height=480 \
    --socket="$weston_socket" --no-config --log="$out_dir/logs/weston.log" \
    >"$out_dir/logs/weston.stdout" 2>&1 &
  weston_pid=$!
  for _ in $(seq 1 50); do [ -S "$waypipe_run/$weston_socket" ] && break; sleep 0.2; done
  if ! kill -0 "$weston_pid" 2>/dev/null || [ ! -S "$waypipe_run/$weston_socket" ]; then
    echo "FATAL: weston did not create $waypipe_run/$weston_socket (log: $out_dir/logs/weston.log)" >&2
    echo "       --debug is required for screenshots (without it weston refuses capture and writes a black PNG)." >&2
    echo "       If the GL renderer could not initialise, retry with --weston-renderer=pixman (explicit opt-in:" >&2
    echo "       the compositor then uses no GPU, so the run is transport evidence only)." >&2
    exit 2
  fi

  # -n/--no-gpu blocks dmabuf, so the guest's buffers travel as wl_shm. The
  # buffer-descriptor failure this mode first recorded is fixed (the guest now
  # publishes the host's real GBM layout as LINEAR), but with dmabuf enabled the
  # presenting Chromium GPU process still aborts and never paints, so the flag
  # stays. See the dmabuf bullet in the README for the measured mechanism.
  env XDG_RUNTIME_DIR="$waypipe_run" WAYLAND_DISPLAY="$weston_socket" \
    "$waypipe_bin" -d -n --socket "$waypipe_sock" client \
    >"$out_dir/logs/waypipe-client.log" 2>&1 &
  waypipe_client_pid=$!
  for _ in $(seq 1 50); do [ -S "$waypipe_sock" ] && break; sleep 0.1; done
  # Check the process too: a stale socket file from a previous run satisfies
  # "-S" even when the client died on EADDRINUSE, which would silently turn the
  # whole transport into a no-op.
  if ! kill -0 "$waypipe_client_pid" 2>/dev/null || [ ! -S "$waypipe_sock" ]; then
    echo "FATAL: the waypipe client is not listening on $waypipe_sock (log: $out_dir/logs/waypipe-client.log)" >&2
    exit 2
  fi
fi

launch_s="$(date +%s)"

loftd_args=(--mem "$mem_gib" --gpu=drm --alloc hardened --seccomp=off --landlock=off)
if [ "$no_default_flag_set" -eq 1 ]; then
  loftd_args=(--mem "$mem_gib")
fi
if [ -n "$guest_init" ]; then
  loftd_args+=(--guest-init "$guest_init")
fi

run_vm() { # run_vm <guest-mode> <console-name>   uses the global vm_args
  local mode="$1" console="$2"
  printf '%s' "$mode" > "$workspace/smoke/run-mode"
  ( cd "$workspace" && env XDG_CONFIG_HOME="$out_dir/config" XDG_STATE_HOME="$state_home" \
      LOFTD_IMAGE="$container_ref" \
      timeout "$timeout_seconds" \
      script -q -e -c "$loftd_bin ${vm_args[*]} -- sh /workspace/smoke/run-guest.sh" /dev/null ) \
    >"$out_dir/logs/$console" 2>&1
}

echo "loftd: $loftd_bin"
echo "image: $container_ref"
echo "backend: $backend | state-home: $state_home"

vm_args=("${loftd_args[@]}")
if [ "$waypipe_mode" -eq 1 ]; then
  vm_args+=(--waypipe="$waypipe_sock")
fi
echo "launch command: $loftd_bin ${vm_args[*]} -- sh /workspace/smoke/run-guest.sh"

if [ "$waypipe_mode" -eq 1 ]; then
  start_watcher "$workspace/evidence/host-frame-early.png" "$workspace/evidence/host-frame-late.png"
fi
if run_vm waypipe loftd.console; then vm_rc=0; else vm_rc=$?; fi

if [ "$vm_rc" -eq 124 ]; then
  echo "FATAL: loftd timed out after ${timeout_seconds}s (log: $out_dir/logs/loftd.console)" >&2
  exit 124
fi
echo "loftd vm exit: $vm_rc (see $out_dir/logs/loftd.console)"

if [ "$waypipe_mode" -eq 1 ]; then
  wait "$watcher_pid" 2>/dev/null || true
  cp "$out_dir/logs/waypipe-client.log" "$workspace/evidence/host-waypipe-client.log" 2>/dev/null || true
  # Attribution control: identical guest work, no --waypipe. Its screenshot must
  # NOT contain the pattern, otherwise a green frame check proves nothing.
  echo "control run: same guest work without --waypipe"
  vm_args=("${loftd_args[@]}")
  start_watcher "$workspace/evidence/control-frame-early.png" "$workspace/evidence/control-frame-late.png"
  if run_vm control loftd-control.console; then control_rc=0; else control_rc=$?; fi
  wait "$watcher_pid" 2>/dev/null || true
  cp "$out_dir/logs/waypipe-client.log" "$workspace/evidence/control-waypipe-client.log" 2>/dev/null || true
  echo "control vm exit: $control_rc (see $out_dir/logs/loftd-control.console)"
fi

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

check version "$E/version.txt" "Chromium version" \
  grep -Eq 'Chromium [0-9]'
check chromium-rc "$E/chromium-rc" "the WebGL and probe-DOM runs exit 0" \
  grep -Eq '^gpu-dom=[0-9]+ webgl=0 dom=0$'

# The GPU/Vulkan assertion: the WebGL probe must report an ANGLE renderer on
# the Vulkan backend that is not the software SwiftShader fallback. This is the
# vulkan/venus evidence. chrome://gpu cannot serve this role: its feature-status
# table is rendered into a custom element's shadow DOM, which --dump-dom does
# not serialize, so gpu-dom.html is captured for humans but not scored.
if fresh "$E/webgl-renderer.txt" \
   && grep -q 'renderer=' "$E/webgl-renderer.txt" \
   && grep -q 'Vulkan' "$E/webgl-renderer.txt" \
   && ! grep -q 'SwiftShader' "$E/webgl-renderer.txt"; then
  echo "PASS  webgl-vulkan (ANGLE/Vulkan renderer, not SwiftShader)"
else
  echo "FAIL  webgl-vulkan (need renderer= with Vulkan and no SwiftShader) -> $E/webgl-renderer.txt"
  fail=1
fi

if [ -s "$E/gpu-dom.html" ]; then
  echo "INFO  gpu-dom rc=$(sed -n 's/^gpu-dom=\([0-9]*\).*/\1/p' "$E/chromium-rc") captured; unscored (feature table is shadow DOM)"
else
  echo "INFO  gpu-dom missing (unscored chrome://gpu dump)"
fi

# PNG: non-empty + magic bytes
if fresh "$E/webgl.png" && [ "$(head -c 4 "$E/webgl.png" | od -An -tx1 | tr -d ' \n')" = "89504e47" ]; then
  echo "PASS  webgl-png (non-empty PNG screenshot)"
else
  echo "FAIL  webgl-png -> $E/webgl.png missing/stale/empty or not a PNG"
  fail=1
fi

# ---- waypipe mode checks ----------------------------------------------------
# The presenting run is the venus authority: it renders on the GPU and presents
# through waypipe at the same time, so its renderer and its frame are the
# strongest evidence this tool can produce. The transport is scored separately
# from the frame so a failure says which half broke.
if [ "$waypipe_mode" -eq 1 ]; then
  if fresh "$E/host-waypipe-client.log" \
     && grep -q 'Connection received' "$E/host-waypipe-client.log" \
     && grep -q 'Connected waypipe-server' "$E/host-waypipe-client.log"; then
    echo "PASS  waypipe-transport (guest waypipe server connected to the host client)"
  else
    echo "FAIL  waypipe-transport (host client log needs 'Connection received' + 'Connected waypipe-server') -> $E/host-waypipe-client.log"
    fail=1
  fi

  # The pattern page publishes the WebGL renderer as the window title, and
  # waypipe logs titles verbatim, so the renderer crosses the transport into a
  # host-side log. No strace (which distorted the run) and no debug port needed.
  grep -aoE 'set_title\("waypipe-venus:[^"]*"' "$E/host-waypipe-client.log" 2>/dev/null \
    | sed -e 's/^set_title("//' -e 's/"$//' | tail -1 > "$E/presenting-renderer.txt" 2>/dev/null || true
  if [ -s "$E/presenting-renderer.txt" ] \
     && grep -q 'Vulkan' "$E/presenting-renderer.txt" \
     && grep -q 'venus' "$E/presenting-renderer.txt" \
     && ! grep -q 'SwiftShader' "$E/presenting-renderer.txt"; then
    echo "PASS  venus-presenting ($(cut -c1-70 "$E/presenting-renderer.txt"))"
  else
    echo "FAIL  venus-presenting (need the presenting run's title to name a venus Vulkan renderer; got: $(head -1 "$E/presenting-renderer.txt" 2>/dev/null))"
    fail=1
  fi

  # Two colours, two claims, one screenshot. #ff00ff is the page's background:
  # it proves the guest's frame reached the compositor (transport). #00ffff is
  # the colour the page prints its renderer=/vendor=/gl_version= overlay in:
  # counting it proves the screenshot a human opens really carries the renderer,
  # so the frame itself supports the hardware-acceleration claim instead of only
  # the host log doing so. The overlay is text, hence the per-channel tolerance
  # for antialiased glyph edges; which text it holds is a human (or OCR) job,
  # and the frame must not be asked to prove more than that - the presenting
  # page cannot draw with WebGL at all (see the README).
  present_px=0
  present_frame=""
  overlay_px=0
  overlay_frame=""
  scored_frame=""
  for f in "$E/host-frame-early.png" "$E/host-frame-late.png"; do
    [ -s "$f" ] || continue
    n="$("$python_bin" "$tool_dir/png-colour-count.py" "$f" ff00ff 2>/dev/null || echo 0)"
    m="$("$python_bin" "$tool_dir/png-colour-count.py" "$f" 00ffff 16 2>/dev/null || echo 0)"
    [ "${n:-0}" -gt "$present_px" ] && { present_px="$n"; present_frame="$f"; }
    [ "${m:-0}" -gt "$overlay_px" ] && { overlay_px="$m"; overlay_frame="$f"; }
    # The frame a human is pointed at should show both claims at once.
    if [ -z "$scored_frame" ] && [ "${n:-0}" -ge 5000 ] && [ "${m:-0}" -ge 500 ]; then
      scored_frame="$f"
    fi
  done
  # Keep the best capture as a top-level artefact: the pixel counts are the
  # score, but a human still has to be able to look at the window that produced
  # them - the frame holds the page's renderer=/vendor= overlay as well.
  if [ -n "$present_frame" ]; then
    cp "${scored_frame:-$present_frame}" "$out_dir/weston-screenshot.png"
  fi
  if [ "$present_px" -ge 5000 ]; then
    echo "PASS  frame-presented (${present_frame##*/} holds $present_px pattern pixels)"
  else
    echo "FAIL  frame-presented (best host screenshot holds $present_px pattern pixels, need >= 5000; the guest's frame never reached the compositor)"
    fail=1
  fi
  if [ "$overlay_px" -ge 500 ]; then
    echo "PASS  renderer-on-frame (${overlay_frame##*/} holds $overlay_px pixels of the page's renderer overlay, so the screenshot names the renderer)"
  else
    echo "FAIL  renderer-on-frame (best host screenshot holds $overlay_px pixels of the overlay colour 00ffff, need >= 500; the presented page did not print its renderer)"
    fail=1
  fi

  # The control's captures must exist: a missing control frame would otherwise
  # read as "no pattern pixels" and pass the check for the wrong reason.
  control_px=0
  control_missing=0
  control_frame=""
  for f in "$E/control-frame-early.png" "$E/control-frame-late.png"; do
    if ! fresh "$f"; then
      control_missing=1
      continue
    fi
    # The control capture is the attribution half: a user comparing it with the
    # presenting frame should see black next to the pattern. Every control frame
    # is expected to be pattern-free, so the first fresh one is as good as any
    # ("best" would be arbitrary - and picking by pattern count would pick none).
    [ -n "$control_frame" ] || control_frame="$f"
    n="$("$python_bin" "$tool_dir/png-colour-count.py" "$f" ff00ff 2>/dev/null || echo 0)"
    [ "${n:-0}" -gt "$control_px" ] && control_px="$n"
  done
  if [ -n "$control_frame" ]; then
    cp "$control_frame" "$out_dir/weston-screenshot-control.png"
  fi
  if [ "$control_missing" -eq 1 ]; then
    echo "FAIL  control-no-frame (the control run produced no fresh compositor screenshot; attribution is unproven)"
    fail=1
  elif [ "$control_px" -lt 5000 ]; then
    echo "PASS  control-no-frame (without --waypipe the compositor screenshot holds $control_px pattern pixels)"
  else
    echo "FAIL  control-no-frame (pattern pixels appeared without --waypipe: $control_px; the frame is not attributable to the transport)"
    fail=1
  fi

  if grep -q 'GL renderer' "$out_dir/logs/weston.log" 2>/dev/null; then
    echo "INFO  compositor $(grep -m1 'GL renderer' "$out_dir/logs/weston.log" | sed 's/^.*] //' | cut -c1-90)"
  elif grep -q 'Using Pixman renderer' "$out_dir/logs/weston.log" 2>/dev/null; then
    echo "INFO  compositor Using Pixman renderer (software compositing; transport evidence only)"
  fi
  echo "INFO  presenting $(head -1 "$E/presenting-waypipe-state.txt" 2>/dev/null | cut -c1-170)"
fi

screenshot_note() { # the frames a human can open to verify the run by eye
  if [ "$waypipe_mode" -eq 1 ]; then
    if [ -s "$out_dir/weston-screenshot.png" ]; then
      echo "  screenshot: $out_dir/weston-screenshot.png (weston frame of the presented guest window)"
    fi
    if [ -s "$out_dir/weston-screenshot-control.png" ]; then
      echo "  screenshot: $out_dir/weston-screenshot-control.png (control: the same guest work without --waypipe)"
    fi
  fi
  return 0
}

if [ "$fail" -eq 0 ]; then
  echo "VERDICT: PASS — evidence: $E (fresh, $(stat -c %Y "$E"/* 2>/dev/null | wc -l) files)"
  screenshot_note
  exit 0
fi

echo "VERDICT: FAIL — inspect:"
echo "  evidence: $E"
screenshot_note
echo "  console:  $out_dir/logs/loftd.console"
echo "  guest chromium logs are mirrored into the evidence dir (chromium-gpu.log, chromium-webgl.log)"
exit 1