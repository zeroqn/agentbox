# Persistent loftd Waypipe and guest exec design

## Summary

Allow a Waypipe-enabled loftd task to start with the normal interactive fish shell instead of requiring a GUI command at VM launch. Add a general foreground `loftd exec` command that starts another process inside any active exec-capable guest. When the guest was launched with `--waypipe`, exec processes inherit the guest's persistent Waypipe display and can launch GUI applications through the existing connection.

Waypipe becomes a VM-level service rather than a wrapper around one command. Arbitrary command execution uses a dedicated host socket, libkrun host-to-guest vsock mapping, guest listener, and versioned protocol. The existing managed-session attach protocol remains unchanged.

## Goals

- Permit `loftd --waypipe=SOCKET` without an explicit guest command.
- Preserve the normal default interactive fish login shell for a commandless Waypipe launch.
- Preserve support for an explicit initial command with `--waypipe`.
- Keep one Waypipe server active for the lifetime of a Waypipe-enabled VM.
- Add `loftd exec <task-selector> -- COMMAND...` for every active exec-capable loftd VM.
- Let exec commands in a Waypipe-enabled VM use its existing Waypipe connection automatically.
- Stream exec stdin, stdout, and stderr in the foreground and return the guest command's exit status.
- Keep exec process lifecycle independent from the primary shell while retaining the primary session as the VM lifetime owner.
- Keep attach and exec as separate protocols and control paths.

## Non-goals

- Detached or background exec management.
- Listing, attaching to, or collecting logs from detached exec processes.
- Allocating a second interactive PTY for exec.
- Running exec commands as root or another selectable identity.
- Per-exec working-directory or environment options.
- Enabling Waypipe after a VM was launched without `--waypipe`.
- Starting SSH, creating the forwarded Waypipe socket, or managing the workstation Waypipe client.
- Changing Waypipe reconnection ownership.
- Combining remote Waypipe with the existing `--wayland` mode or GPU forwarding.

## User interface

### Waypipe launch

A command is no longer required with `--waypipe`:

```bash
loftd --workspace=/home/dev/project --waypipe=/tmp/loftd-waypipe.sock
```

This launches the normal interactive fish login shell. The shell receives the Waypipe display environment and may start GUI applications directly.

An explicit initial command remains supported:

```bash
loftd --workspace=/home/dev/project \
  --waypipe=/tmp/loftd-waypipe.sock \
  -- gui-application
```

The existing requirements remain:

- The Waypipe socket path is absolute.
- The path already exists and is a Unix socket.
- `--waypipe` conflicts with `--wayland` and `--gpu`.
- The first version remains software-only and uses Waypipe `--no-gpu`.

### Existing-task execution

The new command is:

```bash
loftd exec <task-id-or-handle-selector> -- COMMAND...
```

For example:

```bash
loftd ps
loftd exec <task-selector> -- rio
```

The command:

- targets any active exec-capable task, not only Waypipe tasks;
- requires a non-empty argv after `--`;
- executes argv directly without shell parsing;
- runs as guest user `dev`;
- starts in guest `/workspace`;
- inherits the launch-derived guest environment;
- inherits the persistent `WAYLAND_DISPLAY` when the task was launched with `--waypipe`;
- connects host stdin, stdout, and stderr to the guest process;
- forwards termination signals such as `SIGINT` and `SIGTERM` to the guest process group;
- exits with the guest process's exit status.

The first version uses ordinary pipes rather than a PTY. Interactive full-screen terminal applications remain the responsibility of the primary managed session and `loftd attach`.

## Architecture alternatives

### Inject commands into the existing fish PTY

This would write command text into the primary terminal. It has the smallest implementation footprint but is unreliable because behavior depends on current shell state, quoting, aliases, foreground jobs, terminal modes, and prompt readiness. It also cannot cleanly separate output or report an authoritative exit status.

This approach is rejected.

### Extend the attach protocol

This would add process creation and separate stream handling to the existing managed-session socket. It reuses one transport but mixes two different abstractions: attachment to one persistent PTY and execution of independent processes. This increases protocol complexity and creates regression risk for terminal replay, detach, resize, and single-attacher behavior.

This approach is rejected.

### Persistent Waypipe service and dedicated exec service

This design starts Waypipe as a VM-level service and creates a separate exec control path. Attach continues to represent the primary PTY only. Exec represents one independent process per client connection.

This approach is selected because it establishes clear service boundaries, supports general execution, and leaves the mature attach path unchanged.

## Launch contract

Every newly launched managed task receives exec metadata:

- a dedicated guest vsock port;
- an exec protocol version;
- a task-private host Unix socket path;
- a libkrun host-to-guest mapping from that Unix socket to the guest port.

A Waypipe-enabled launch additionally receives:

- the existing Waypipe guest vsock port;
- the host's existing SSH-forwarded Unix socket path;
- a fixed guest Wayland display name, initially `loftd-waypipe-0`.

The launch-config codec carries the guest exec port and protocol version to guest-init. The host-only exec socket path remains part of host launch and active-task state, following the existing managed attach split.

The active-task record stores exec capability metadata alongside managed attach metadata. Older records remain readable and simply have no exec capability.

## Guest bootstrap and supervision

Guest-init performs the existing one-time bootstrap first:

- validates prepared-root paths;
- prepares networking and storage services;
- resolves the `dev` identity;
- derives and exports the normal shell environment;
- applies hardening and allocator settings;
- starts optional guest services such as the local Wayland proxy.

For a Waypipe-enabled task, guest-init then starts one persistent process equivalent to:

```bash
waypipe --no-gpu \
  --vsock \
  --socket <guest-port> \
  --display loftd-waypipe-0 \
  server -- sleep infinity
```

The fixed display name makes the guest socket location deterministic under the existing `XDG_RUNTIME_DIR`. Guest-init waits for that Wayland socket to become ready before starting user processes. It then adds `WAYLAND_DISPLAY=loftd-waypipe-0` to the environment used by both the primary process and subsequent exec processes.

Guest-init starts the exec listener before starting the primary process. The long-lived guest supervisor owns:

- the primary managed PTY process;
- the persistent Waypipe process when enabled;
- the exec listener;
- all active exec process groups.

The primary process remains the task lifetime owner. When it exits, guest-init terminates active exec children and the Waypipe service, then exits with the primary process's status. Exiting or failing an exec process does not affect the primary process or VM.

Unexpected exit of the persistent Waypipe process is fatal for a Waypipe-enabled task because the VM can no longer provide the capability declared at launch. Guest-init terminates the remaining task processes and reports the failure through the existing task lifecycle.

## Exec transport

The host allocates a task-private Unix socket in the active task directory. Libkrun maps that socket to the dedicated guest exec port in the host-to-guest direction, matching the existing attach transport pattern while using a separate port and socket.

The guest listens on the exec vsock port. Each accepted connection represents exactly one foreground process. Multiple connections may be active concurrently.

The host flow is:

1. Resolve the task selector with the existing active-task selector logic.
2. Verify that the task is active and has exec metadata.
3. Connect to the task's exec Unix socket.
4. Negotiate the exec protocol version.
5. Send the argv request.
6. Proxy stdin and signals to the guest.
7. Receive stdout, stderr, structured errors, and final exit status.
8. Return the guest exit status from `loftd exec`.

The guest flow is:

1. Accept one exec connection.
2. Validate the protocol greeting and request.
3. Fork a child process group.
4. Set the child identity to `dev`.
5. Set the working directory to `/workspace`.
6. Apply the immutable environment snapshot produced during bootstrap.
7. Attach separate stdin, stdout, and stderr pipes.
8. Execute the argv directly.
9. Forward output frames and accept input or signal frames until exit.
10. Send the final exit status and reap the process.

The guest listener handles independent connections concurrently. Implementation may use one thread per active connection because exec concurrency is expected to be small and bounded by local users of the task socket; no asynchronous runtime is introduced solely for this feature.

## Exec protocol

The exec protocol is independent from `loftd-attach-protocol` and has its own version constant. A small shared protocol crate may be introduced if needed by both host and guest crates, following the existing attach-protocol pattern.

Required message semantics are:

- host-to-guest hello with protocol version;
- guest-to-host hello acknowledgement or version error;
- host-to-guest start request containing a length-delimited argv vector;
- host-to-guest stdin data;
- host-to-guest stdin EOF;
- host-to-guest signal request;
- guest-to-host stdout data;
- guest-to-host stderr data;
- guest-to-host structured startup or runtime error;
- guest-to-host final exit status.

Argv is never assembled into a shell command. Length-delimited fields prevent quoting ambiguity and command injection.

The protocol sets conservative frame-size and argv-size bounds, consistent with the existing launch and attach protocol style. Exceeding a boundary returns a protocol error without affecting the VM.

## Signal and disconnect behavior

The host forwards termination-oriented signals received while `loftd exec` is active, including `SIGINT` and `SIGTERM`, to the guest exec process group. This lets Ctrl-C terminate a foreground GUI or console process without terminating the VM or primary shell.

If the host connection disappears before the process exits, the guest terminates that exec process group and reaps it. Foreground exec therefore does not leave accidental orphan processes after terminal loss or client failure.

A normal stdin EOF closes only the guest process's stdin; it does not terminate the process automatically.

## Environment and working directory

Guest bootstrap produces one environment snapshot after identity resolution, home materialization, service setup, and optional Waypipe readiness. Both the primary process and all exec processes receive this snapshot.

This includes existing launch-derived values such as:

- `HOME` and user identity variables;
- image `PATH` and shell environment;
- Nix and container-storage variables;
- allocator selection;
- terminal-independent task environment;
- `WAYLAND_DISPLAY=loftd-waypipe-0` only for Waypipe-enabled tasks.

Exec does not copy the mutable environment or current directory of the interactive fish process. That state belongs to the shell and is not a reliable VM-wide contract. Every exec process starts in `/workspace`.

## Task-record compatibility

The active-task record format adds optional exec metadata containing the host socket and protocol version. Decoding an older record yields `exec: None`.

`loftd exec` against an active legacy task returns a clear error that the task does not support exec and must be relaunched with a current loftd version. `ps`, `attach`, and `kill` continue to work with the record according to their existing compatibility behavior.

Launch-config codec changes follow the repository's existing append-only compatibility rules. New guest exec fields are optional when decoding older launch contracts and required only when the new managed exec capability is enabled.

## Error handling

The host rejects before connecting when:

- the exec command is empty;
- the task selector is unknown or ambiguous;
- the task record is stale;
- the task has no exec capability;
- the exec socket cannot be reached.

The guest returns a structured error when:

- protocol versions are incompatible;
- the request is malformed or exceeds limits;
- the working directory cannot be selected;
- pipes or process-group setup fail;
- identity dropping fails;
- the executable cannot be started.

These errors affect only the exec request. They do not terminate the VM unless the failure is in a VM-level service such as the persistent Waypipe process or exec listener itself.

For Waypipe launch, guest-init must not start the primary shell with a declared but unusable display. Failure to start Waypipe or observe the display socket produces a clear startup failure.

## Security boundaries

- The host exec socket is inside the existing task-private state directory and follows its ownership and permissions.
- The exec mapping is host-to-guest only and is not exposed as TCP or guest network service.
- The host accepts no remote unauthenticated exec connection.
- Exec always runs as `dev`; there is no root or UID override.
- Requests carry argv fields rather than shell text.
- Existing task-wide seccomp, Landlock, mounts, allocator, networking, and guest-kernel policies apply to exec children.
- The SSH reverse-forwarded Waypipe socket remains externally owned. loftd connects to it but does not create, unlink, replace, or clean it up.

## Testing

### CLI tests

- `--waypipe` without a command selects the default fish command.
- `--waypipe -- COMMAND...` remains accepted.
- `exec` requires a selector and non-empty command.
- `exec` command arguments may begin with hyphens after `--`.

### Launch and state tests

- Exec port and protocol metadata round-trip through launch config.
- Exec host socket metadata round-trips through active-task records.
- Older launch contracts and active-task records decode without exec capability.
- Waypipe display metadata is present only for Waypipe-enabled launches.
- Attach, Waypipe, and exec use distinct sockets and guest ports.

### Protocol tests

- Version negotiation succeeds and rejects mismatches.
- Argv vectors round-trip without shell parsing.
- stdin, stdout, and stderr frames remain distinct.
- stdin EOF and exit status are represented correctly.
- Signal and error frames validate their payloads.
- Oversized and malformed frames are rejected.

### Guest tests

- Persistent Waypipe command construction uses `--no-gpu`, vsock, fixed display, and a lifetime command.
- The primary process starts only after the Waypipe display socket is ready.
- Primary and exec processes receive the same Waypipe display environment.
- Exec runs as `dev` in `/workspace`.
- Exec preserves stdout/stderr separation and exit status.
- Signals target the exec process group.
- Client disconnect cleans up the exec process group.
- Multiple exec requests can run concurrently.
- Primary-session exit cleans up exec children and Waypipe.
- Exec failure does not terminate the primary session.

### Host tests

- Existing selector resolution is reused for exec.
- Legacy non-exec tasks return the intended error.
- Host stream proxying propagates exit status and signals.
- Libkrun receives a separate host-to-guest mapping for the exec socket.
- Existing attach and Waypipe mappings remain unchanged.

### Documentation and image checks

- Update `README.md` to describe commandless Waypipe launch and later GUI launch through `loftd exec`.
- Preserve the image assertion that the loftd image contains Waypipe.
- No new image package is required for exec itself.

## Validation

Run the standard repository checks:

```bash
nix develop --command cargo fmt --check
nix develop --command cargo clippy --all-targets --all-features -- -D warnings
nix develop --command cargo deny check
nix develop --command cargo test
```

Also build the relevant Nix packages and image checks.

Live validation should:

1. Start the workstation Waypipe client and authenticated SSH reverse Unix-socket forwarding.
2. Launch `loftd --waypipe=<socket>` without a command and confirm fish appears.
3. Resolve the task with `loftd ps`.
4. Run a GUI application with `loftd exec <task> -- <application>` and confirm it appears on the workstation.
5. Confirm the primary fish shell remains usable while and after exec.
6. Confirm stdout, stderr, exit status, Ctrl-C, and client-disconnect cleanup.
7. Start concurrent exec commands.
8. Exit the primary shell and confirm the VM, Waypipe process, and active exec children are cleaned up.

## Approved decisions

- `loftd exec` is general to all active exec-capable VMs.
- Exec is foreground-only in the first version.
- Exec uses non-PTY streams.
- Exec runs as `dev` in `/workspace` with the launch-derived environment.
- Waypipe is a persistent VM-level service with a stable display name.
- Exec uses a dedicated protocol and socket rather than extending attach.
- The primary managed session remains the VM lifetime owner.
