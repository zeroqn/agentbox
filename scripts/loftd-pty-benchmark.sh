#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
iterations=3
warmup=1
live_iterations=0
live_warmup=0
out_dir=""
loftd_bin="${LOFTD_BIN:-$repo_root/result/bin/loftd}"
use_cargo_run=0
skip_live=0
live_failed=0
rmux_bin="${RMUX_BIN:-}"
tmux_bin="${TMUX_BIN:-}"
timeout_seconds=180
loftd_extra_args=()
default_live_mem_gib=2
live_state_home="${LOFTD_BENCH_XDG_STATE_HOME:-}"
live_guest_init="${LOFTD_BENCH_GUEST_INIT:-}"
default_live_guest_init=1
require_guest_profile=0

usage() {
  cat <<'USAGE'
Usage: loftd-pty-benchmark.sh [OPTIONS]

Run reproducible PTY benchmark diagnostics for loftd attach-loop latency. The
runner records synthetic PTY baselines, a required live loftd run with
LOFTD_ATTACH_PROFILE=1, and optional rmux/tmux comparison hooks when available.

Options:
      --loftd <path>          loftd binary to run (default: $LOFTD_BIN or ./result/bin/loftd)
      --loftd-cargo-run       run loftd as: cargo run -p loftd -- (explicit opt-in)
      --out-dir <path>        output directory (default: .omx/benchmarks/loftd-pty/<timestamp>)
      --iterations <n>        measured iterations per workload (default: 3)
      --warmup <n>            warmup iterations per synthetic workload (default: 1)
      --live-iterations <n>   measured iterations for opt-in live redraw+typing scenario (default: 0)
      --live-warmup <n>       warmup iterations for opt-in live redraw+typing scenario (default: 0)
      --rmux <path>           optional rmux binary for comparison hook
      --tmux <path>           optional tmux binary for isolated comparison hook
      --skip-live             skip required live loftd run (for synthetic development smoke only)
      --timeout <seconds>     live command timeout (default: 180)
      --loftd-arg <arg>       repeatable extra argument passed before the guest command
      --no-default-live-mem  do not add the benchmark default --mem 2 live-run arg
      --state-home <path>    XDG_STATE_HOME for live loftd (default: btrfs containers disk when needed)
      --guest-init <path>    guest init path for live loftd (default: ./result/bin/loftd-guest-init if present)
      --no-default-guest-init
                              do not auto-add the default guest init override
      --require-guest-profile
                              fail unless the live run captures a guest profile line
  -h, --help                  show this help

Artifacts:
  metrics.jsonl               per-run machine-readable records
  summary.json                aggregate summary and live profile objects
  logs/*.stdout, *.stderr     raw captured command output

Completion evidence for loftd PTY work should not use --skip-live: live runs
must capture the host "loftd attach profile" summary. Guest summaries are
captured when the guest/libkrun console is visible; use --require-guest-profile
for strict guest-profile diagnostics.

The default live run remains the single live-loftd-shell smoke/profile capture.
Pass --live-iterations <n> to add the live-loftd-redraw-typing scenario, which
drives stdin markers through loftd while the guest emits redraw bursts and
output markers; records include hot-window and marker-latency evidence.
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --loftd) loftd_bin="${2:?missing value for --loftd}"; use_cargo_run=0; shift 2 ;;
    --loftd-cargo-run) use_cargo_run=1; shift ;;
    --out-dir) out_dir="${2:?missing value for --out-dir}"; shift 2 ;;
    --iterations) iterations="${2:?missing value for --iterations}"; shift 2 ;;
    --warmup) warmup="${2:?missing value for --warmup}"; shift 2 ;;
    --live-iterations) live_iterations="${2:?missing value for --live-iterations}"; shift 2 ;;
    --live-warmup) live_warmup="${2:?missing value for --live-warmup}"; shift 2 ;;
    --rmux) rmux_bin="${2:?missing value for --rmux}"; shift 2 ;;
    --tmux) tmux_bin="${2:?missing value for --tmux}"; shift 2 ;;
    --skip-live) skip_live=1; shift ;;
    --timeout) timeout_seconds="${2:?missing value for --timeout}"; shift 2 ;;
    --loftd-arg) loftd_extra_args+=("${2:?missing value for --loftd-arg}"); shift 2 ;;
    --no-default-live-mem) default_live_mem_gib=""; shift ;;
    --state-home) live_state_home="${2:?missing value for --state-home}"; shift 2 ;;
    --guest-init) live_guest_init="${2:?missing value for --guest-init}"; shift 2 ;;
    --no-default-guest-init) default_live_guest_init=0; shift ;;
    --require-guest-profile) require_guest_profile=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 1 ;;
  esac
done

for value_name in iterations warmup live_iterations live_warmup timeout_seconds; do
  value="${!value_name}"
  case "$value" in ''|*[!0-9]*) echo "--${value_name/_/-} must be a non-negative integer" >&2; exit 1 ;; esac
done
if [ "$timeout_seconds" -eq 0 ]; then
  echo "--timeout must be greater than zero" >&2
  exit 1
fi

if [ -z "$out_dir" ]; then
  out_dir="$repo_root/.omx/benchmarks/loftd-pty/$(date -u +%Y%m%dT%H%M%SZ)"
fi
mkdir -p "$out_dir/logs"
metrics_path="$out_dir/metrics.jsonl"
summary_path="$out_dir/summary.json"
: > "$metrics_path"

if ! command -v python3 >/dev/null 2>&1; then
  echo "missing required command: python3" >&2
  exit 1
fi

if [ -z "$rmux_bin" ]; then
  if command -v rmux >/dev/null 2>&1; then
    rmux_bin="$(command -v rmux)"
  elif [ -x /mnt/rmux/target/release/rmux ]; then
    rmux_bin="/mnt/rmux/target/release/rmux"
  elif [ -x /mnt/rmux/target/debug/rmux ]; then
    rmux_bin="/mnt/rmux/target/debug/rmux"
  fi
fi
if [ -z "$tmux_bin" ] && command -v tmux >/dev/null 2>&1; then
  tmux_bin="$(command -v tmux)"
fi

if [ "$use_cargo_run" -eq 1 ]; then
  command -v cargo >/dev/null 2>&1 || { echo "--loftd-cargo-run requires cargo" >&2; exit 1; }
  loftd_display="cargo run -p loftd --"
elif [ -x "$loftd_bin" ]; then
  loftd_display="$loftd_bin"
else
  echo "loftd binary is not executable: $loftd_bin" >&2
  echo "pass --loftd <path>, set LOFTD_BIN, or use --loftd-cargo-run" >&2
  exit 1
fi

python3 - "$metrics_path" "$iterations" "$warmup" <<'PY'
import json, os, pty, selectors, signal, subprocess, sys, termios, time
metrics_path, iterations, warmup = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])

def set_raw(fd):
    attrs = termios.tcgetattr(fd)
    attrs[0] &= ~(termios.IGNBRK | termios.BRKINT | termios.PARMRK | termios.ISTRIP | termios.INLCR | termios.IGNCR | termios.ICRNL | termios.IXON)
    attrs[1] &= ~termios.OPOST
    attrs[2] |= termios.CS8
    attrs[3] &= ~(termios.ECHO | termios.ECHONL | termios.ICANON | termios.ISIG | termios.IEXTEN)
    termios.tcsetattr(fd, termios.TCSANOW, attrs)

def run_pty_command(command, payload, expect_bytes, timeout=5.0):
    master, slave = pty.openpty()
    set_raw(slave)
    proc = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=slave, close_fds=True, start_new_session=True)
    os.close(slave)
    selector = selectors.DefaultSelector()
    selector.register(master, selectors.EVENT_READ)
    started = time.perf_counter_ns()
    bytes_out = 0
    error = None
    try:
        os.write(master, payload)
        deadline = time.monotonic() + timeout
        while bytes_out < expect_bytes and time.monotonic() < deadline:
            events = selector.select(max(0.0, min(0.05, deadline - time.monotonic())))
            for _key, _mask in events:
                try:
                    data = os.read(master, 65536)
                except OSError as exc:
                    error = str(exc)
                    data = b""
                if data:
                    bytes_out += len(data)
    finally:
        elapsed_us = (time.perf_counter_ns() - started) // 1000
        try: os.close(master)
        except OSError: pass
        if proc.poll() is None:
            try: os.killpg(proc.pid, signal.SIGTERM)
            except ProcessLookupError: pass
            try: proc.wait(timeout=1)
            except subprocess.TimeoutExpired:
                try: os.killpg(proc.pid, signal.SIGKILL)
                except ProcessLookupError: pass
                proc.wait(timeout=1)
        else:
            proc.wait(timeout=1)
    return ("ok" if error is None and bytes_out >= expect_bytes else "failed"), elapsed_us, bytes_out, error

def cat_payload():
    return b"loftd-pty-benchmark-cat-echo-0123456789abcdef\n" * 128

def redraw_payload():
    return b"".join(f"\x1b[2K\rloftd redraw frame {i:03d} cursor-snap probe".encode() for i in range(160)) + b"\n"

with open(metrics_path, "a", encoding="utf-8") as metrics:
    for scenario, command, payload in [
        ("synthetic-cat-echo", ["cat"], cat_payload()),
        ("synthetic-redraw-burst", ["cat"], redraw_payload()),
    ]:
        for ordinal in range(warmup + iterations):
            iteration = ordinal - warmup
            status, elapsed_us, bytes_out, error = run_pty_command(command, payload, len(payload))
            if iteration < 0:
                continue
            metrics.write(json.dumps({
                "schema_version": 1, "scenario": scenario, "mode": "synthetic", "iteration": iteration,
                "status": status, "elapsed_us": elapsed_us, "bytes_in": len(payload), "bytes_out": bytes_out,
                "artifact_stdout": None, "artifact_stderr": None, "profile_role": None, "profile": {},
                "skip_reason": None, "error": error,
            }, sort_keys=True) + "\n")
PY

default_live_state_home() {
  if [ -n "$live_state_home" ]; then
    printf '%s\n' "$live_state_home"
    return 0
  fi
  local containers_root="/home/dev/.local/share/containers"
  if [ -d "$containers_root" ] && [ -w "$containers_root" ]; then
    local slug
    slug="$(basename "$out_dir" | tr -c 'A-Za-z0-9_.-' '-')"
    printf '%s\n' "$containers_root/loftd-pty-benchmark-state/$slug"
    return 0
  fi
  if [ -n "${XDG_STATE_HOME:-}" ]; then
    printf '%s\n' "$XDG_STATE_HOME"
    return 0
  fi
  return 1
}

loftd_args_contain_mem() {
  local arg
  for arg in "${loftd_extra_args[@]}"; do
    case "$arg" in
      --mem|--mem=*) return 0 ;;
    esac
  done
  return 1
}

loftd_args_contain_guest_init() {
  local arg
  for arg in "${loftd_extra_args[@]}"; do
    case "$arg" in
      --guest-init|--guest-init=*) return 0 ;;
    esac
  done
  return 1
}

default_live_guest_init_path() {
  if [ -n "$live_guest_init" ]; then
    printf '%s\n' "$live_guest_init"
    return 0
  fi
  if [ "$default_live_guest_init" -eq 1 ] && [ -x "$repo_root/result/bin/loftd-guest-init" ]; then
    printf '%s\n' "$repo_root/result/bin/loftd-guest-init"
    return 0
  fi
  return 1
}

append_default_live_args() {
  if [ -n "$default_live_mem_gib" ] && ! loftd_args_contain_mem; then
    printf '%s\0%s\0' --mem "$default_live_mem_gib"
  fi
  if ! loftd_args_contain_guest_init; then
    local guest_init_path
    guest_init_path="$(default_live_guest_init_path || true)"
    if [ -n "$guest_init_path" ]; then
      printf '%s\0%s\0' --guest-init "$guest_init_path"
    fi
  fi
}

append_skip() {
  python3 - "$metrics_path" "$1" "$2" "$3" <<'PY'
import json, sys
path, scenario, mode, reason = sys.argv[1:5]
record = {
    "schema_version": 1, "scenario": scenario, "mode": mode, "iteration": 0, "status": "skipped",
    "elapsed_us": 0, "bytes_in": 0, "bytes_out": 0, "artifact_stdout": None, "artifact_stderr": None,
    "profile_role": None, "profile": {}, "skip_reason": reason, "error": None,
}
with open(path, "a", encoding="utf-8") as metrics:
    metrics.write(json.dumps(record, sort_keys=True) + "\n")
PY
}


run_optional_rmux_attach_drain() {
  python3 - "$metrics_path" "$rmux_bin" "$out_dir" "$iterations" "$warmup" <<'PY'
import fcntl
import json
import os
import pathlib
import pty
import selectors
import signal
import struct
import subprocess
import sys
import termios
import time

metrics_path = sys.argv[1]
rmux_bin = sys.argv[2]
out_dir = pathlib.Path(sys.argv[3])
iterations = int(sys.argv[4])
warmup = int(sys.argv[5])
logs_dir = out_dir / "logs"
logs_dir.mkdir(parents=True, exist_ok=True)
scenario = "optional-rmux"
mode = "rmux_attach_drain"
read_timeout_seconds = 10.0


def write_record(record):
    with open(metrics_path, "a", encoding="utf-8") as metrics:
        metrics.write(json.dumps(record, sort_keys=True) + "\n")


def base_record(iteration, stdout_path=None, stderr_path=None):
    return {
        "schema_version": 1,
        "scenario": scenario,
        "mode": mode,
        "iteration": max(iteration, 0),
        "elapsed_us": 0,
        "bytes_in": 0,
        "bytes_out": 0,
        "artifact_stdout": str(stdout_path) if stdout_path else None,
        "artifact_stderr": str(stderr_path) if stderr_path else None,
        "profile_role": "rmux_attach_drain",
        "profile": {},
        "skip_reason": None,
        "error": None,
    }


def rmux(label, *args, timeout=5.0, check=False):
    result = subprocess.run(
        [rmux_bin, "-L", label, *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=timeout,
        check=False,
    )
    if check and result.returncode != 0:
        stderr = result.stderr.strip() or result.stdout.strip() or f"exit status {result.returncode}"
        raise RuntimeError(stderr)
    return result


def cleanup(label):
    kill = rmux(label, "kill-server", timeout=5.0, check=False)
    probe = rmux(label, "list-sessions", timeout=2.0, check=False)
    return kill.returncode == 0 or probe.returncode != 0


def attach_child(label, session):
    pid, fd = pty.fork()
    if pid == 0:
        os.execlp(rmux_bin, rmux_bin, "-L", label, "attach-session", "-t", session)
    try:
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 120, 0, 0))
    except OSError:
        pass
    return pid, fd


def stop_child(pid, fd):
    try:
        os.close(fd)
    except OSError:
        pass
    try:
        os.kill(pid, signal.SIGTERM)
    except OSError:
        pass
    try:
        os.waitpid(pid, os.WNOHANG)
    except (ChildProcessError, OSError):
        pass


def run_sample(ordinal, measured_iteration):
    measured = measured_iteration >= 0
    label = f"loftd-pty-bench-{os.getpid()}-{ordinal}-{time.time_ns()}"
    session = "bench"
    done_token = f"RMUX_ATTACH_DRAIN_DONE_{os.getpid()}_{ordinal}".encode()
    stdout_path = logs_dir / f"optional-rmux-attach-drain-{measured_iteration}.stdout" if measured else None
    stderr_path = logs_dir / f"optional-rmux-attach-drain-{measured_iteration}.stderr" if measured else None
    output = bytearray()
    stderr_lines = []
    read_times = []
    attach_pid = None
    attach_fd = None
    elapsed_us = 0
    error = None
    setup_complete = False
    cleanup_ok = False
    started_ns = time.perf_counter_ns()

    workload = (
        "i=0; "
        "while [ $i -lt 160 ]; do "
        "printf '\\033[2K\\rrmux-attach-drain-%03d cursor-snap-probe' \"$i\"; "
        "i=$((i + 1)); "
        "done; "
        f"printf '\\n{done_token.decode()}\\n'"
    )

    try:
        cleanup(label)
        rmux(label, "new-session", "-d", "-s", session, "-x", "120", "-y", "30", "/bin/sh", timeout=5.0, check=True)
        setup_complete = True
        started_ns = time.perf_counter_ns()
        attach_pid, attach_fd = attach_child(label, session)
        selector = selectors.DefaultSelector()
        selector.register(attach_fd, selectors.EVENT_READ)
        # Let the attached client subscribe before timing the finite payload drain.
        time.sleep(0.10)
        started_ns = time.perf_counter_ns()
        sent = rmux(
            label,
            "send-keys",
            "-t",
            session,
            workload,
            "Enter",
            timeout=5.0,
            check=False,
        )
        if sent.returncode != 0:
            raise RuntimeError(sent.stderr.strip() or sent.stdout.strip() or f"send-keys exit status {sent.returncode}")
        deadline = time.monotonic() + read_timeout_seconds
        saw_done = False
        while time.monotonic() < deadline:
            events = selector.select(max(0.0, min(0.05, deadline - time.monotonic())))
            if not events:
                child, _status = os.waitpid(attach_pid, os.WNOHANG)
                if child == attach_pid:
                    attach_pid = None
                    break
                continue
            for _key, _mask in events:
                try:
                    data = os.read(attach_fd, 65536)
                except OSError:
                    data = b""
                if not data:
                    continue
                output.extend(data)
                read_times.append(time.perf_counter_ns())
                if done_token in output:
                    saw_done = True
                    break
            if saw_done:
                break
        elapsed_us = (time.perf_counter_ns() - started_ns) // 1000
        if not saw_done:
            error = f"timed out waiting for {done_token.decode()}"
    except Exception as exc:
        elapsed_us = (time.perf_counter_ns() - started_ns) // 1000
        error = str(exc)
        stderr_lines.append(str(exc))
    finally:
        if attach_pid is not None and attach_fd is not None:
            stop_child(attach_pid, attach_fd)
        elif attach_fd is not None:
            try:
                os.close(attach_fd)
            except OSError:
                pass
        try:
            cleanup_ok = cleanup(label)
        except Exception as exc:
            cleanup_ok = False
            stderr_lines.append(f"cleanup failed: {exc}")

    if stdout_path:
        stdout_path.write_bytes(bytes(output))
    if stderr_path:
        stderr_text = "\n".join(line for line in stderr_lines if line)
        stderr_path.write_text(
            stderr_text + ("\n" if stderr_text else ""),
            encoding="utf-8",
        )

    if not measured:
        return

    gaps_us = [
        (read_times[i] - read_times[i - 1]) // 1000
        for i in range(1, len(read_times))
    ]
    gap_avg_us = int(sum(gaps_us) / len(gaps_us)) if gaps_us else 0
    gap_max_us = max(gaps_us) if gaps_us else 0
    status = "ok" if error is None and cleanup_ok else "failed"
    if error is None and not cleanup_ok:
        error = "rmux cleanup verification failed"
    record = base_record(measured_iteration, stdout_path, stderr_path)
    record.update({
        "status": status,
        "elapsed_us": elapsed_us,
        "bytes_in": len(workload.encode()),
        "bytes_out": len(output),
        "error": error,
        "profile": {
            "rmux_binary": rmux_bin,
            "socket_name": label,
            "session_name": session,
            "setup_complete": setup_complete,
            "attach_bytes_drained": len(output),
            "attach_read_count": len(read_times),
            "attach_read_gap_avg_us": gap_avg_us,
            "attach_read_gap_max_us": gap_max_us,
            "cleanup_ok": cleanup_ok,
        },
    })
    write_record(record)


for ordinal in range(warmup + iterations):
    run_sample(ordinal, ordinal - warmup)
PY
}

run_live_loftd() {
  local stdout_path="$out_dir/logs/live-loftd-shell.stdout"
  local stderr_path="$out_dir/logs/live-loftd-shell.stderr"
  local status_path="$out_dir/logs/live-loftd-shell.status"
  local -a command_prefix
  local -a effective_loftd_args
  local -a default_args
  effective_loftd_args=("${loftd_extra_args[@]}")
  mapfile -d '' -t default_args < <(append_default_live_args)
  if [ "${#default_args[@]}" -gt 0 ]; then
    effective_loftd_args=("${default_args[@]}" "${effective_loftd_args[@]}")
  fi
  if [ "$use_cargo_run" -eq 1 ]; then
    command_prefix=(cargo run -p loftd --)
  else
    command_prefix=("$loftd_bin")
  fi
  local workload_host_path="$out_dir/live-workload.sh"
  cat > "$workload_host_path" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
i=0
while [ "$i" -lt 160 ]; do
  printf '\033[2K\rloftd-live-pty-benchmark-%03d cursor-snap-probe' "$i"
  i=$((i + 1))
done
printf '\n'
SH
  chmod +x "$workload_host_path"

  local workload_host_abs
  workload_host_abs="$(cd "$(dirname "$workload_host_path")" && pwd)/$(basename "$workload_host_path")"
  local workload_guest_path
  case "$workload_host_abs" in
    "$repo_root"/*)
      workload_guest_path="/workspace/${workload_host_abs#"$repo_root"/}"
      ;;
    *)
      echo "live workload path must be under repo root for the guest to see it: $workload_host_abs" >&2
      return 1
      ;;
  esac

  local -a full_command=("${command_prefix[@]}" "${effective_loftd_args[@]}" -- bash "$workload_guest_path")
  local loftd_effective_display="$loftd_display"
  if [ "${#effective_loftd_args[@]}" -gt 0 ]; then
    printf -v loftd_effective_display '%q ' "$loftd_display" "${effective_loftd_args[@]}"
    loftd_effective_display="${loftd_effective_display% }"
  fi

  local effective_state_home
  effective_state_home="$(default_live_state_home || true)"
  if [ -n "$effective_state_home" ]; then
    mkdir -p "$effective_state_home"
  fi

  python3 - "$metrics_path" "$stdout_path" "$stderr_path" "$status_path" "$timeout_seconds" "$loftd_effective_display" "$effective_state_home" "$require_guest_profile" -- "${full_command[@]}" <<'PY'
import fcntl, json, os, pathlib, pty, re, selectors, signal, struct, sys, termios, time
metrics_path, stdout_path, stderr_path, status_path, timeout_seconds, loftd_display, state_home, require_guest_profile = sys.argv[1:9]
command = sys.argv[10:]
timeout_seconds = int(timeout_seconds)
require_guest_profile = require_guest_profile == "1"
stdout_file, stderr_file, status_file = map(pathlib.Path, (stdout_path, stderr_path, status_path))
env = os.environ.copy()
env["LOFTD_ATTACH_PROFILE"] = "1"
if state_home:
    env["XDG_STATE_HOME"] = state_home

pid, master = pty.fork()
if pid == 0:
    try:
        os.execvpe(command[0], command, env)
    except Exception as exc:
        os.write(2, f"failed to exec {command[0]}: {exc}\n".encode())
        os._exit(127)
try:
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 120, 0, 0))
except OSError:
    pass

selector = selectors.DefaultSelector()
selector.register(master, selectors.EVENT_READ)
started = time.perf_counter_ns()
chunks = []
timed_out = False
wait_status = None
try:
    deadline = time.monotonic() + timeout_seconds
    while True:
        waited_pid, status = os.waitpid(pid, os.WNOHANG)
        if waited_pid == pid:
            wait_status = status
            quiet_deadline = time.monotonic() + 0.25
            hard_drain_deadline = time.monotonic() + 2.0
            while time.monotonic() < hard_drain_deadline:
                events = selector.select(0.05)
                if not events:
                    if time.monotonic() >= quiet_deadline:
                        break
                    continue
                quiet_deadline = time.monotonic() + 0.25
                for _key, _mask in events:
                    try: data = os.read(master, 65536)
                    except OSError:
                        data = b""
                    if data:
                        chunks.append(data)
            break
        if time.monotonic() >= deadline:
            timed_out = True
            try: os.killpg(pid, signal.SIGTERM)
            except OSError:
                try: os.kill(pid, signal.SIGTERM)
                except ProcessLookupError: pass
            try: _, wait_status = os.waitpid(pid, 0)
            except ChildProcessError: wait_status = None
            break
        for _key, _mask in selector.select(0.05):
            try: data = os.read(master, 65536)
            except OSError: data = b""
            if data: chunks.append(data)
finally:
    elapsed_us = (time.perf_counter_ns() - started) // 1000
    try: os.close(master)
    except OSError: pass

if wait_status is None:
    exit_status = -signal.SIGTERM
elif os.WIFEXITED(wait_status):
    exit_status = os.WEXITSTATUS(wait_status)
elif os.WIFSIGNALED(wait_status):
    exit_status = -os.WTERMSIG(wait_status)
else:
    exit_status = 1

combined_bytes = b"".join(chunks)
combined = combined_bytes.decode(errors="replace")
stdout_file.write_text(combined, encoding="utf-8", errors="replace")
stderr_file.write_text("", encoding="utf-8")
status_file.write_text(str(exit_status) + "\n", encoding="utf-8")
roles = []
for match in re.finditer(r"loftd attach profile role=(host|guest)([^\r\n]*)", combined):
    role = match.group(1)
    profile = {"role": role}
    for token in match.group(2).split():
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=[^\s]+", token):
            break
        key, value = token.split("=", 1)
        value = value.rstrip(",;")
        if re.fullmatch(r"-?\d+", value): parsed = int(value)
        elif re.fullmatch(r"-?\d+\.\d+", value): parsed = float(value)
        else: parsed = value
        profile[key] = parsed
    roles.append((role, profile))

with open(metrics_path, "a", encoding="utf-8") as metrics:
    base = {
        "schema_version": 1, "scenario": "live-loftd-shell", "mode": "live_loftd", "iteration": 0,
        "elapsed_us": elapsed_us, "bytes_in": 0, "bytes_out": len(combined_bytes),
        "artifact_stdout": stdout_path, "artifact_stderr": stderr_path, "skip_reason": None,
        "loftd_command": loftd_display,
        "env": {"LOFTD_ATTACH_PROFILE": "1", "XDG_STATE_HOME": state_home or env.get("XDG_STATE_HOME")},
    }
    if roles:
        for role, profile in roles:
            record = dict(base)
            record.update({"status": "ok" if exit_status == 0 else "failed", "profile_role": role, "profile": profile, "error": None if exit_status == 0 else f"loftd exited with status {exit_status}"})
            metrics.write(json.dumps(record, sort_keys=True) + "\n")
    else:
        reason = "timed out" if timed_out else "missing loftd attach profile summaries"
        record = dict(base)
        record.update({"status": "failed", "profile_role": None, "profile": {}, "error": f"{reason}; loftd exit status {exit_status}"})
        metrics.write(json.dumps(record, sort_keys=True) + "\n")
required_roles = {"host"}
if require_guest_profile:
    required_roles.add("guest")
missing = required_roles - {role for role, _ in roles}
if exit_status != 0:
    raise SystemExit(f"live loftd failed with status {exit_status}; see {stdout_path}")
if missing:
    raise SystemExit(f"missing live loftd profile role(s): {', '.join(sorted(missing))}; see {stdout_path}")
if not require_guest_profile and "guest" not in {role for role, _ in roles}:
    print(
        "warning: live run did not capture a guest profile; pass --loftd-arg --log-level "
        "--loftd-arg debug plus --require-guest-profile for strict guest diagnostics",
        file=sys.stderr,
    )
PY
}

run_live_loftd_redraw_typing() {
  if [ "$live_iterations" -eq 0 ]; then
    append_skip "live-loftd-redraw-typing" "live_loftd_redraw_typing" "--live-iterations is 0; opt-in live interactive scenario not requested"
    return 0
  fi

  local -a command_prefix
  local -a effective_loftd_args
  local -a default_args
  effective_loftd_args=("${loftd_extra_args[@]}")
  mapfile -d '' -t default_args < <(append_default_live_args)
  if [ "${#default_args[@]}" -gt 0 ]; then
    effective_loftd_args=("${default_args[@]}" "${effective_loftd_args[@]}")
  fi
  if [ "$use_cargo_run" -eq 1 ]; then
    command_prefix=(cargo run -p loftd --)
  else
    command_prefix=("$loftd_bin")
  fi

  local workload_host_path="$out_dir/live-redraw-typing-workload.sh"
  cat > "$workload_host_path" <<'SH'
#!/usr/bin/env bash
set -euo pipefail

sample="${1:?missing sample}"
marker_count="${2:?missing marker count}"
redraw_frames_per_marker="${3:?missing redraw frames per marker}"

printf 'LOFTD_PTY_READY_%s\n' "$sample"
i=0
while [ "$i" -lt "$marker_count" ]; do
  if ! IFS= read -r line; then
    break
  fi
  expected="LOFTD_PTY_INPUT_${sample}_${i}"
  if [ "$line" != "$expected" ]; then
    printf 'LOFTD_PTY_UNEXPECTED_%s_%d\n' "$sample" "$i"
    continue
  fi
  frame=0
  while [ "$frame" -lt "$redraw_frames_per_marker" ]; do
    printf '\033[2K\rloftd-live-redraw-typing-%s-%02d-%03d cursor-snap-probe' "$sample" "$i" "$frame"
    frame=$((frame + 1))
  done
  printf '\nLOFTD_PTY_OUTPUT_%s_%d\n' "$sample" "$i"
  i=$((i + 1))
done
printf 'LOFTD_PTY_DONE_%s\n' "$sample"
SH
  chmod +x "$workload_host_path"

  local workload_host_abs
  workload_host_abs="$(cd "$(dirname "$workload_host_path")" && pwd)/$(basename "$workload_host_path")"
  local workload_guest_path
  case "$workload_host_abs" in
    "$repo_root"/*)
      workload_guest_path="/workspace/${workload_host_abs#"$repo_root"/}"
      ;;
    *)
      echo "live redraw+typing workload path must be under repo root for the guest to see it: $workload_host_abs" >&2
      return 1
      ;;
  esac

  local loftd_effective_display="$loftd_display"
  if [ "${#effective_loftd_args[@]}" -gt 0 ]; then
    printf -v loftd_effective_display '%q ' "$loftd_display" "${effective_loftd_args[@]}"
    loftd_effective_display="${loftd_effective_display% }"
  fi

  local effective_state_home
  effective_state_home="$(default_live_state_home || true)"
  if [ -n "$effective_state_home" ]; then
    mkdir -p "$effective_state_home"
  fi

  local marker_count=8
  local redraw_frames_per_marker=20
  local -a full_command_base=("${command_prefix[@]}" "${effective_loftd_args[@]}" -- bash "$workload_guest_path")
  python3 - "$metrics_path" "$out_dir" "$timeout_seconds" "$loftd_effective_display" "$effective_state_home" "$require_guest_profile" "$live_iterations" "$live_warmup" "$marker_count" "$redraw_frames_per_marker" -- "${full_command_base[@]}" <<'PY'
import json
import os
import pathlib
import pty
import re
import selectors
import signal
import statistics
import struct
import sys
import termios
import time
import fcntl

(
    metrics_path,
    out_dir,
    timeout_seconds,
    loftd_display,
    state_home,
    require_guest_profile,
    live_iterations,
    live_warmup,
    marker_count,
    redraw_frames_per_marker,
) = sys.argv[1:11]
command_base = sys.argv[12:]
out_dir = pathlib.Path(out_dir)
logs_dir = out_dir / "logs"
timeout_seconds = int(timeout_seconds)
require_guest_profile = require_guest_profile == "1"
live_iterations = int(live_iterations)
live_warmup = int(live_warmup)
marker_count = int(marker_count)
redraw_frames_per_marker = int(redraw_frames_per_marker)


def percentile(values, q):
    if not values:
        return 0
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(len(ordered) * q))]


def parse_profiles(combined):
    roles = []
    for match in re.finditer(r"loftd attach profile role=(host|guest)([^\r\n]*)", combined):
        role = match.group(1)
        profile = {"role": role}
        for token in match.group(2).split():
            if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=[^\s]+", token):
                break
            key, value = token.split("=", 1)
            value = value.rstrip(",;")
            if re.fullmatch(r"-?\d+", value):
                parsed = int(value)
            elif re.fullmatch(r"-?\d+\.\d+", value):
                parsed = float(value)
            else:
                parsed = value
            profile[key] = parsed
        roles.append((role, profile))
    return roles


def write_record(record):
    with open(metrics_path, "a", encoding="utf-8") as metrics:
        metrics.write(json.dumps(record, sort_keys=True) + "\n")


def exit_status_from_wait(wait_status):
    if wait_status is None:
        return -signal.SIGTERM
    if os.WIFEXITED(wait_status):
        return os.WEXITSTATUS(wait_status)
    if os.WIFSIGNALED(wait_status):
        return -os.WTERMSIG(wait_status)
    return 1


def stop_child(pid):
    try:
        os.killpg(pid, signal.SIGTERM)
    except OSError:
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass


def run_sample(ordinal, measured_iteration):
    measured = measured_iteration >= 0
    sample = f"{os.getpid()}_{ordinal}_{time.time_ns()}"
    stdout_path = logs_dir / f"live-loftd-redraw-typing-{measured_iteration}.stdout" if measured else None
    stderr_path = logs_dir / f"live-loftd-redraw-typing-{measured_iteration}.stderr" if measured else None
    status_path = logs_dir / f"live-loftd-redraw-typing-{measured_iteration}.status" if measured else None
    command = command_base + [sample, str(marker_count), str(redraw_frames_per_marker)]
    env = os.environ.copy()
    env["LOFTD_ATTACH_PROFILE"] = "1"
    if state_home:
        env["XDG_STATE_HOME"] = state_home

    ready_token = f"LOFTD_PTY_READY_{sample}"
    done_token = f"LOFTD_PTY_DONE_{sample}"
    output_tokens = [f"LOFTD_PTY_OUTPUT_{sample}_{idx}" for idx in range(marker_count)]
    input_tokens = [f"LOFTD_PTY_INPUT_{sample}_{idx}" for idx in range(marker_count)]
    pid, master = pty.fork()
    if pid == 0:
        try:
            os.execvpe(command[0], command, env)
        except Exception as exc:
            os.write(2, f"failed to exec {command[0]}: {exc}\n".encode())
            os._exit(127)
    try:
        fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 120, 0, 0))
    except OSError:
        pass

    selector = selectors.DefaultSelector()
    selector.register(master, selectors.EVENT_READ)
    started_ns = time.perf_counter_ns()
    hot_started_ns = None
    hot_finished_ns = None
    chunks = []
    read_times = []
    marker_send_ns = {}
    marker_seen_ns = {}
    next_to_send = 0
    timed_out = False
    wait_status = None
    error = None
    try:
        deadline = time.monotonic() + timeout_seconds
        while True:
            waited_pid, status = os.waitpid(pid, os.WNOHANG)
            if waited_pid == pid:
                wait_status = status
                quiet_deadline = time.monotonic() + 0.25
                hard_drain_deadline = time.monotonic() + 2.0
                while time.monotonic() < hard_drain_deadline:
                    events = selector.select(0.05)
                    if not events:
                        if time.monotonic() >= quiet_deadline:
                            break
                        continue
                    quiet_deadline = time.monotonic() + 0.25
                    for _key, _mask in events:
                        try:
                            data = os.read(master, 65536)
                        except OSError:
                            data = b""
                        if data:
                            chunks.append(data)
                            read_times.append(time.perf_counter_ns())
                break
            if time.monotonic() >= deadline:
                timed_out = True
                error = "timed out waiting for live redraw+typing workload completion"
                stop_child(pid)
                try:
                    _, wait_status = os.waitpid(pid, 0)
                except ChildProcessError:
                    wait_status = None
                break
            events = selector.select(0.05)
            if not events:
                continue
            for _key, _mask in events:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    data = b""
                if not data:
                    continue
                now_ns = time.perf_counter_ns()
                chunks.append(data)
                read_times.append(now_ns)
                text = b"".join(chunks).decode(errors="replace")
                if hot_started_ns is None and ready_token in text:
                    hot_started_ns = now_ns
                    os.write(master, (input_tokens[next_to_send] + "\n").encode())
                    marker_send_ns[next_to_send] = time.perf_counter_ns()
                    next_to_send += 1
                for idx, token in enumerate(output_tokens):
                    if idx not in marker_seen_ns and token in text:
                        marker_seen_ns[idx] = now_ns
                        if next_to_send < marker_count:
                            os.write(master, (input_tokens[next_to_send] + "\n").encode())
                            marker_send_ns[next_to_send] = time.perf_counter_ns()
                            next_to_send += 1
                if done_token in text and hot_finished_ns is None:
                    hot_finished_ns = now_ns
    finally:
        total_elapsed_us = (time.perf_counter_ns() - started_ns) // 1000
        try:
            os.close(master)
        except OSError:
            pass

    combined_bytes = b"".join(chunks)
    combined = combined_bytes.decode(errors="replace")
    final_observed_ns = read_times[-1] if read_times else started_ns
    if hot_started_ns is None and ready_token in combined:
        hot_started_ns = final_observed_ns
    for idx, token in enumerate(output_tokens):
        if idx not in marker_seen_ns and idx in marker_send_ns and token in combined:
            marker_seen_ns[idx] = final_observed_ns
    if hot_finished_ns is None and done_token in combined:
        hot_finished_ns = final_observed_ns
    exit_status = exit_status_from_wait(wait_status)
    roles = parse_profiles(combined)
    role_profiles = {}
    for role, profile in roles:
        role_profiles.setdefault(role, []).append(profile)
    saw_ready = hot_started_ns is not None or ready_token in combined
    saw_done = hot_finished_ns is not None or done_token in combined
    if hot_finished_ns is None and saw_done and read_times:
        hot_finished_ns = read_times[-1]
    marker_latencies_us = [
        (marker_seen_ns[idx] - marker_send_ns[idx]) // 1000
        for idx in range(marker_count)
        if idx in marker_seen_ns and idx in marker_send_ns
    ]
    gaps_us = [
        (read_times[idx] - read_times[idx - 1]) // 1000
        for idx in range(1, len(read_times))
    ]
    hot_window_elapsed_us = (
        (hot_finished_ns - hot_started_ns) // 1000
        if hot_started_ns is not None and hot_finished_ns is not None and hot_finished_ns >= hot_started_ns
        else None
    )
    if error is None and exit_status != 0:
        error = f"loftd exited with status {exit_status}"
    if error is None and not saw_ready:
        error = f"missing ready token {ready_token}"
    if error is None and not saw_done:
        error = f"missing done token {done_token}"
    if error is None and len(marker_seen_ns) != marker_count:
        error = f"observed {len(marker_seen_ns)} of {marker_count} output markers"
    if error is None and "host" not in role_profiles:
        error = "missing loftd attach profile role(s): host"
    if error is None and require_guest_profile and "guest" not in role_profiles:
        error = "missing loftd attach profile role(s): guest"

    if stdout_path:
        stdout_path.write_text(combined, encoding="utf-8", errors="replace")
    if stderr_path:
        stderr_path.write_text("", encoding="utf-8")
    if status_path:
        status_path.write_text(str(exit_status) + "\n", encoding="utf-8")
    if not measured:
        return

    record = {
        "schema_version": 1,
        "scenario": "live-loftd-redraw-typing",
        "mode": "live_loftd_redraw_typing",
        "iteration": measured_iteration,
        "status": "ok" if error is None else "failed",
        "elapsed_us": total_elapsed_us,
        "bytes_in": sum(len(token) + 1 for token in input_tokens[:next_to_send]),
        "bytes_out": len(combined_bytes),
        "artifact_stdout": str(stdout_path),
        "artifact_stderr": str(stderr_path),
        "profile_role": "live_redraw_typing",
        "skip_reason": None,
        "error": error,
        "loftd_command": loftd_display,
        "env": {"LOFTD_ATTACH_PROFILE": "1", "XDG_STATE_HOME": state_home or env.get("XDG_STATE_HOME")},
        "profile": {
            "total_elapsed_us": total_elapsed_us,
            "hot_window_elapsed_us": hot_window_elapsed_us,
            "marker_count": marker_count,
            "markers_seen": len(marker_seen_ns),
            "marker_latencies_us": marker_latencies_us,
            "marker_latency_min_us": min(marker_latencies_us) if marker_latencies_us else 0,
            "marker_latency_avg_us": int(statistics.fmean(marker_latencies_us)) if marker_latencies_us else 0,
            "marker_latency_p50_us": int(statistics.median(sorted(marker_latencies_us))) if marker_latencies_us else 0,
            "marker_latency_p95_us": percentile(marker_latencies_us, 0.95),
            "marker_latency_max_us": max(marker_latencies_us) if marker_latencies_us else 0,
            "read_count": len(read_times),
            "read_gap_avg_us": int(statistics.fmean(gaps_us)) if gaps_us else 0,
            "read_gap_max_us": max(gaps_us) if gaps_us else 0,
            "redraw_frames_per_marker": redraw_frames_per_marker,
            "bytes_drained": len(combined_bytes),
            "saw_ready": saw_ready,
            "saw_done": saw_done,
            "attach_profiles": role_profiles,
        },
    }
    write_record(record)


for ordinal in range(live_warmup + live_iterations):
    run_sample(ordinal, ordinal - live_warmup)
PY
}

if [ "$skip_live" -eq 1 ]; then
  append_skip "live-loftd-shell" "skip" "--skip-live was passed; live loftd evidence not collected"
  append_skip "live-loftd-redraw-typing" "skip" "--skip-live was passed; live loftd evidence not collected"
else
  run_live_loftd || live_failed=$?
  if [ "$live_failed" -eq 0 ]; then
    run_live_loftd_redraw_typing || live_failed=$?
  elif [ "$live_iterations" -eq 0 ]; then
    append_skip "live-loftd-redraw-typing" "live_loftd_redraw_typing" "--live-iterations is 0; opt-in live interactive scenario not requested"
  fi
fi

if [ -n "$rmux_bin" ] && [ -x "$rmux_bin" ]; then
  run_optional_rmux_attach_drain
else
  append_skip "optional-rmux" "rmux" "rmux binary not found or not executable; /mnt/rmux left read-only"
fi
if [ -n "$tmux_bin" ] && [ -x "$tmux_bin" ]; then
  append_skip \
    "optional-tmux" \
    "tmux" \
    "tmux comparison hook detected $tmux_bin; isolated tmux workload intentionally skipped in initial benchmark runner"
else
  append_skip "optional-tmux" "tmux" "tmux binary not found or not executable"
fi

loftd_summary_display="$loftd_display"
summary_effective_loftd_args=("${loftd_extra_args[@]}")
summary_default_loftd_args=()
mapfile -d '' -t summary_default_loftd_args < <(append_default_live_args)
if [ "${#summary_default_loftd_args[@]}" -gt 0 ]; then
  summary_effective_loftd_args=("${summary_default_loftd_args[@]}" "${summary_effective_loftd_args[@]}")
fi
if [ "${#summary_effective_loftd_args[@]}" -gt 0 ]; then
  printf -v loftd_summary_display '%q ' "$loftd_display" "${summary_effective_loftd_args[@]}"
  loftd_summary_display="${loftd_summary_display% }"
fi

summary_state_home="$(default_live_state_home || true)"
python3 - "$metrics_path" "$summary_path" "$out_dir" "$loftd_summary_display" "$skip_live" "$iterations" "$warmup" "$live_iterations" "$live_warmup" "$summary_state_home" "$require_guest_profile" <<'PY'
import json, pathlib, statistics, sys
(
    metrics_path,
    summary_path,
    out_dir,
    loftd_display,
    skip_live,
    iterations,
    warmup,
    live_iterations,
    live_warmup,
    state_home,
    require_guest_profile,
) = sys.argv[1:12]
require_guest_profile = require_guest_profile == "1"
records = [json.loads(line) for line in open(metrics_path, encoding="utf-8") if line.strip()]
by_scenario = {}
profiles = {"host": [], "guest": [], "rmux_attach_drain": []}
skips, failures = [], []
for record in records:
    by_scenario.setdefault(record["scenario"], []).append(record)
    if record.get("profile_role") in profiles:
        profiles[record["profile_role"]].append(record.get("profile", {}))
    if record.get("status") == "skipped":
        skips.append({"scenario": record["scenario"], "reason": record.get("skip_reason")})
    if record.get("status") == "failed":
        failures.append({"scenario": record["scenario"], "error": record.get("error")})
scenarios = {}
for scenario, items in by_scenario.items():
    elapsed = [
        item["elapsed_us"]
        for item in items
        if item.get("status") == "ok" and item.get("elapsed_us") is not None
    ]
    if not elapsed:
        scenarios[scenario] = {"count": 0}
        continue
    sorted_elapsed = sorted(elapsed)
    scenarios[scenario] = {
        "count": len(elapsed),
        "min_elapsed_us": min(elapsed),
        "avg_elapsed_us": int(statistics.fmean(elapsed)),
        "max_elapsed_us": max(elapsed), "p50_elapsed_us": int(statistics.median(sorted_elapsed)),
        "p95_elapsed_us": sorted_elapsed[min(len(sorted_elapsed) - 1, int(len(sorted_elapsed) * 0.95))],
    }

def percentile(values, q):
    if not values:
        return 0
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(len(ordered) * q))]

scenario_profiles = {}
redraw_typing_profiles = [
    record.get("profile", {})
    for record in by_scenario.get("live-loftd-redraw-typing", [])
    if record.get("status") == "ok" and record.get("profile")
]
if redraw_typing_profiles:
    hot_windows = [
        profile["hot_window_elapsed_us"]
        for profile in redraw_typing_profiles
        if profile.get("hot_window_elapsed_us") is not None
    ]
    marker_latencies = [
        latency
        for profile in redraw_typing_profiles
        for latency in profile.get("marker_latencies_us", [])
    ]
    read_counts = [profile.get("read_count") or 0 for profile in redraw_typing_profiles]
    read_gap_avgs = [profile.get("read_gap_avg_us") or 0 for profile in redraw_typing_profiles]
    scenario_profiles["live-loftd-redraw-typing"] = {
        "sample_count": len(redraw_typing_profiles),
        "marker_count_total": sum(profile.get("marker_count") or 0 for profile in redraw_typing_profiles),
        "markers_seen_total": sum(profile.get("markers_seen") or 0 for profile in redraw_typing_profiles),
        "hot_window_min_us": min(hot_windows) if hot_windows else 0,
        "hot_window_avg_us": int(statistics.fmean(hot_windows)) if hot_windows else 0,
        "hot_window_p50_us": int(statistics.median(sorted(hot_windows))) if hot_windows else 0,
        "hot_window_p95_us": percentile(hot_windows, 0.95),
        "hot_window_max_us": max(hot_windows) if hot_windows else 0,
        "marker_latency_min_us": min(marker_latencies) if marker_latencies else 0,
        "marker_latency_avg_us": int(statistics.fmean(marker_latencies)) if marker_latencies else 0,
        "marker_latency_p50_us": int(statistics.median(sorted(marker_latencies))) if marker_latencies else 0,
        "marker_latency_p95_us": percentile(marker_latencies, 0.95),
        "marker_latency_max_us": max(marker_latencies) if marker_latencies else 0,
        "read_count_total": sum(read_counts),
        "read_count_avg": int(statistics.fmean(read_counts)) if read_counts else 0,
        "read_gap_avg_us": int(statistics.fmean(read_gap_avgs)) if read_gap_avgs else 0,
        "read_gap_max_us": max((profile.get("read_gap_max_us") or 0 for profile in redraw_typing_profiles), default=0),
        "bytes_drained_total": sum(profile.get("bytes_drained") or 0 for profile in redraw_typing_profiles),
    }
summary = {
    "schema_version": 1,
    "out_dir": out_dir,
    "metrics_path": metrics_path,
    "loftd_command": loftd_display,
    "live_required": skip_live != "1",
    "iterations": int(iterations),
    "warmup": int(warmup),
    "live_iterations": int(live_iterations),
    "live_warmup": int(live_warmup),
    "live_env": {"LOFTD_ATTACH_PROFILE": "1", "XDG_STATE_HOME": state_home or None},
    "profile_requirements": {"host": skip_live != "1", "guest": require_guest_profile},
    "profile_warnings": [],
    "scenarios": scenarios, "scenario_profiles": scenario_profiles,
    "profiles": profiles, "skips": skips, "failures": failures,
}
if skip_live != "1" and profiles["host"] and not profiles["guest"]:
    summary["profile_warnings"].append(
        "guest profile not captured; guest summaries currently require visible guest/libkrun console output"
    )
pathlib.Path(summary_path).write_text(
    json.dumps(summary, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
print(f"loftd PTY benchmark artifacts: {out_dir}")
for scenario, stats in sorted(scenarios.items()):
    if stats.get("count"):
        print(
            f"{scenario}: count={stats['count']} "
            f"min={stats['min_elapsed_us']}us "
            f"avg={stats['avg_elapsed_us']}us "
            f"p50={stats['p50_elapsed_us']}us "
            f"p95={stats['p95_elapsed_us']}us "
            f"max={stats['max_elapsed_us']}us"
        )
    else:
        print(f"{scenario}: no successful measured runs")
if profiles["host"]:
    host = profiles["host"][-1]
    print(
        f"host profile: frames={host.get('frames')} "
        f"stdout_batches={host.get('stdout_batches')} "
        f"stdout_write_count={host.get('stdout_write_count')} "
        f"stdout_flush_count={host.get('stdout_flush_count')}"
    )
if profiles["guest"]:
    guest = profiles["guest"][-1]
    print(
        f"guest profile: pty_reads={guest.get('pty_reads')} "
        f"pty_drain_events={guest.get('pty_drain_events')} "
        f"parser_total_us={guest.get('parser_total_us')} "
        f"frame_write_total_us={guest.get('frame_write_total_us')}"
    )
if profiles["rmux_attach_drain"]:
    rmux_profiles = profiles["rmux_attach_drain"]
    bytes_total = sum(profile.get("attach_bytes_drained") or 0 for profile in rmux_profiles)
    reads_total = sum(profile.get("attach_read_count") or 0 for profile in rmux_profiles)
    max_gap = max(
        (profile.get("attach_read_gap_max_us") or 0 for profile in rmux_profiles),
        default=0,
    )
    cleanup_ok = all(bool(profile.get("cleanup_ok")) for profile in rmux_profiles)
    print(
        f"rmux attach-drain profile: samples={len(rmux_profiles)} "
        f"read_count={reads_total} bytes={bytes_total} "
        f"max_gap={max_gap}us cleanup_ok={cleanup_ok}"
    )
if scenario_profiles.get("live-loftd-redraw-typing"):
    live_profile = scenario_profiles["live-loftd-redraw-typing"]
    print(
        f"live redraw-typing profile: samples={live_profile['sample_count']} "
        f"markers={live_profile['markers_seen_total']}/{live_profile['marker_count_total']} "
        f"hot_avg={live_profile['hot_window_avg_us']}us "
        f"marker_p95={live_profile['marker_latency_p95_us']}us "
        f"read_count={live_profile['read_count_total']} "
        f"max_gap={live_profile['read_gap_max_us']}us"
    )
for warning in summary["profile_warnings"]:
    print(f"warning: {warning}", file=sys.stderr)
for skip in skips:
    print(f"skipped {skip['scenario']}: {skip['reason']}")
for failure in failures:
    print(f"failed {failure['scenario']}: {failure['error']}", file=sys.stderr)
PY

if [ "$live_failed" -ne 0 ]; then
  exit "$live_failed"
fi
