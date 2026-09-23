# Usage reference

Day-to-day loftd command reference beyond the [README](../README.md) quick
start and command list.

## `loftd exec`

`loftd exec <task-id-or-handle-selector> -- COMMAND...` runs a non-PTY foreground
command in an active task with separate stdin, stdout, and stderr streams. It
uses the task's `/workspace` directory and returns the guest command's exit
status. Tasks launched by older loftd versions do not have the exec transport;
relaunch them with the current version before using `loftd exec`.

## Sessions: detach and attach

- A normal foreground `loftd` run starts a managed guest PTY session and then
  attaches the host terminal to it. The foreground experience is still an
  interactive shell or command, but the guest process is not tied to the host
  terminal lifetime. Managed guest PTY sessions preserve the launching host
  terminal identity by passing non-empty UTF-8 `TERM`, `COLORTERM`,
  `TERM_PROGRAM`, and `TERM_PROGRAM_VERSION` values into the guest; this is
  limited to the managed attach path and does not enable broad host environment
  passthrough. The guest init also defaults missing or empty `LANG` and
  `LC_CTYPE` to `C.UTF-8` so locale-sensitive terminal programs such as `tmux`
  can use UTF-8 character widths, including CJK text. Explicit locale values
  are preserved, `LC_ALL` is not set, and this does not broaden host
  environment passthrough. When the managed guest reports an exit status, loftd
  propagates that status as the foreground process result; a guest status such
  as 127 is distinct from loftd helper or VM infrastructure failure diagnostics.
  Managed helper diagnostics are mediated by the parent after terminal raw mode
  is restored, so infrastructure errors start on a fresh terminal line instead
  of racing with guest PTY output. Detached or `--preserve-debug` sessions keep
  helper stderr in the task state directory as `helper.stderr.log` for later
  inspection; normal managed cleanup removes it with the task state.
- Press `Ctrl-\` twice to detach from the current terminal
  session. loftd recognizes both raw `Ctrl-\` bytes and CSI-u/Kitty-encoded
  `Ctrl-\` events from terminals or multiplexers. The host-side filter
  intercepts the sequence before it reaches the guest TTY, where `Ctrl-\`
  would otherwise be the POSIX quit character. Closing the host terminal,
  killing the attach client, or losing SSH also behaves as detach: the guest
  shell or terminal-interactive command keeps running while the VM helper
  remains active.
- `loftd --daemon` starts the managed guest PTY through the launching terminal,
  forwards startup input/output so the target program can complete terminal
  initialization, then detaches automatically after the first target output is
  followed by a short idle window. The heuristic is generic and does not parse
  shell prompts. This mode is TTY-only; if stdin or stdout is not a terminal,
  loftd fails before sending the attach frame that starts the target program.
  Use `loftd attach <task-id-or-handle-selector>` (or
  `loftd a <task-id-or-handle-selector>`) to reconnect and
  `loftd kill <task-id-or-handle-selector>` to terminate the detached task.
- Reconnect with `loftd attach <task-id-or-handle-selector>` or its `loftd a`
  shortcut. Selectors follow the same task-id/handle matching rules as
  `loftd kill`; use `loftd ps` to list running task IDs and handles. Reattach
  repaints the current visible terminal screen from bounded in-memory guest PTY
  state before forwarding new output, so a detached shell or TUI should be
  usable without pressing `Enter` just to
  redraw. This restore state is not persisted across helper or VM restart.
- Only one attach client is supported at a time. A second attach attempt receives
  a busy error instead of sharing the PTY.
- Terminal and TUI programs inside the PTY are in scope, including
  `loftd -- <interactive-command>`. Graphical X11/Wayland application
  preservation is not implemented; display sockets and GUI reconnect semantics
  need a separate design.
- The attach transport is libkrun's vsock-to-host-Unix-socket mapping. If the
  required `krun_add_vsock_port2` symbol or setup path is unavailable, managed
  attach fails clearly instead of falling back to another transport. On slow or
  heavily loaded hosts, managed attach readiness can need a longer guest boot
  window before the guest sends its initial `Hello` frame. Increase the bounded
  readiness windows with:

  ```bash
  LOFTD_MANAGED_ATTACH_READY_TIMEOUT_SECS=180 \
  LOFTD_MANAGED_HELPER_READY_TIMEOUT_SECS=190 \
  ./result/bin/loftd -- bash -lc 'echo ok'
  ```

  `LOFTD_MANAGED_ATTACH_READY_TIMEOUT_SECS` controls how long the helper waits
  for the guest attach listener to complete the initial `Hello` handshake.
  `LOFTD_MANAGED_HELPER_READY_TIMEOUT_SECS` controls how long the parent waits
  for the helper to report readiness.
- Exiting the guest shell or command terminates the VM and removes the active
  task/rootfs unless `--preserve-debug` was used. Detached tasks can be
  terminated with `loftd kill <task-id-or-handle-selector>`.

## Task control: `loftd ps` and `loftd kill`

Active task control is loftd-native and does not use host Podman as a runtime
backend:

```bash
loftd ps
loftd kill <task-id-or-handle-selector>
```

`loftd ps` scans loftd's app state and lists active task VM records across all
workspaces by default. The human-readable table includes a short handle, full task
id, status, helper PID/session identity, start timestamp, image, and workspace
slug. For a full task id like `loftd-4138-178109091122334455`, the handle is
`loftd-4138`, so `loftd kill loftd-4138` can target that task without typing the
opaque suffix. You can also use a displayed-handle prefix of at least two
characters, such as `loftd kill lo`, when that prefix uniquely matches one
visible handle. For handles shaped like `<name>-<number>`, `loftd kill` also
accepts `<name-prefix>-<handle-number-prefix>` when it uniquely matches a
displayed handle; for example, `loftd kill lo-18` can target
`loftd-1845-<opaque>` through its displayed handle `loftd-1845`. The
handle-number prefix is the numeric segment shown in the displayed handle, not
the helper process PID. Prefix matching is only against displayed handles, not
full task ids. It is an active-task view only: completed task history, log
inspection, JSON/API output, restart/pause/exec operations, and Podman-backed
management are intentionally out of scope.
`loftd kill <task-id-or-handle-selector>` validates the recorded process and
session identity before signaling the task process group, sends `SIGTERM`, waits
briefly, and escalates to `SIGKILL` only if the task is still running. Ambiguous
handles or handle selectors, too-short prefixes, malformed abbreviated
selectors, reused process ids, or unreadable process identities are reported
instead of signaled. Stale records for already-exited tasks are eligible for a
cleanup retry without signaling. A successful kill request only returns after
the task rootfs/state cleanup succeeds, then removes the active record from
subsequent `ps` output. If cleanup fails after the VM process is gone, `loftd
kill` returns a visible error and leaves or restores the active record so rerun
`loftd kill <task-id-or-handle-selector>` can retry the same cleanup.

## Volumes

Use repeatable `-v, --volume SOURCE:TARGET[:ro|:rw]` to add host bind mounts
to the prepared root. `SOURCE` may be a host file or directory; relative
sources are resolved from the workspace. `TARGET` must be an absolute guest
path. Omitting the mode defaults to read-write, `:rw` is explicit read-write,
and `:ro` remounts the bind target read-only after grafting:

```bash
./result/bin/loftd -v /host/cache:/home/dev/project-cache -- bash -lc 'ls /home/dev/project-cache'
./result/bin/loftd --volume /host/config.json:/workspace/config.json:ro -- cat /workspace/config.json
```

User volumes are additive only: they do not replace `/workspace`, `/nix`, or
the built-in tool config/state, compiler-cache, and container-store mounts, and
duplicate guest targets are rejected. Loftd intentionally does not support Podman SELinux
suffixes (`:z`, `:Z`), ownership mutation (`:U`), propagation flags, named
volumes, or anonymous volumes.

## Root shell handoff

```bash
./result/bin/loftd --root
# inside the root shell:
loftd-as-dev          # execs fish -l as dev
loftd-as-dev id -un  # runs a command as dev
```

`loftd-as-dev` is packaged only in the loftd image. It is a narrow root-only
helper for dropping from an interactive loftd root shell back to the materialized
`dev` account. With no arguments it launches `fish -l`; with arguments it runs
that command as `dev`. Exiting that fish or command returns to the invoking root
shell only when the helper was started as a child process from an interactive
root shell. The helper does not provide sudo/su, does not switch arbitrary users,
and cannot be used by `dev` to regain root.

## Container-store maintenance

```bash
./result/bin/loftd container-store resize --size 128G
./result/bin/loftd container-store reset --force
```

These commands manage only the current workspace's `loftd-containers.raw` disk
used by the raw-disk container store. They do not inspect or migrate any legacy
host-directory container store and do not resize or reset loftd's host `/nix`
overlay state.
`resize` is grow-only: `--size` accepts bytes or binary suffixes such as `K`,
`M`, `G`, `T`, `KiB`, `MiB`, `GiB`, and `TiB`, and the requested size must be
larger than the current raw file. It grows the host sparse file first, then
starts a narrow one-shot direct-libkrun maintenance VM that runs
`loftd-guest-init internal resize containers` to expand the guest btrfs
filesystem. If that guest resize fails after the host file has grown, loftd does
not shrink or roll back the file; fix the reported VM/guest problem and rerun
the same resize command.

`reset` is destructive and requires `--force`. It refuses to run while the
current workspace has running, pid-reused, unreadable, or unscannable task
records, deletes an existing regular `loftd-containers.raw`, and recreates the
default 64 GiB sparse btrfs image without launching a VM. Stale-only task
records are reported as cleanup information and do not block either command.
For a manual smoke test on a host with Buildah, btrfs-progs, and libkrun
available, run `loftd --container-store raw-disk` once, then run the `resize`
and `reset --force` commands above.

## Guest-init override

For loftd guest-side debugging, `--guest-init <host-binary>` validates the
host binary as an executable regular file, discovers the image's existing
`/nix/store/.../bin/loftd-guest-init`, and bind-mounts the host binary
read-only over that exact in-image target. Loftd still execs the discovered
`/nix/store/.../bin/loftd-guest-init` guest path and preserves the same
`LOFTD_*`, `KRUN_CONFIG`, arguments, and final guest command; it does not copy
or chmod the task-rootfs `/nix/store` file.
