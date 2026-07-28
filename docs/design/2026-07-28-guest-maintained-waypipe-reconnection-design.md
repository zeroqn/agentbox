# Restartable Waypipe sessions for exec

## Summary

Waypipe is a launch-time task capability. A task launched with `--waypipe` owns a guest Waypipe server on the stable display `loftd-waypipe-0` and task-lifetime host transport sockets.

Two exec forms have distinct behavior:

```bash
loftd --waypipe exec TASK -- GUI_APP
loftd --waypipe=/path/to/client.sock exec TASK -- GUI_APP
```

The valueless form reuses the running Waypipe server. The valued form changes the external target, terminates and reaps the running server, starts a fresh server on the same display name, waits for readiness, and then starts the command.

Replacement is not Waypipe protocol reconnection. Existing GUI applications connected to the replaced server lose their Wayland connection and normally exit. This design uses the current Rust Waypipe implementation and does not depend on the legacy C `waypipe recon` command.

## CLI contract

### Launch

```bash
loftd --waypipe
loftd --waypipe=/path/to/client.sock
loftd --waypipe=/path/to/client.sock -- GUI_APP
```

- `--waypipe` enables the task capability without an initial target.
- `--waypipe=SOCKET` enables the capability and selects an initial absolute Unix socket target.
- Without an explicit command, the normal interactive fish login shell starts.
- Waypipe remains mutually exclusive with local `--wayland` and GPU forwarding modes.

### Exec

```bash
loftd exec TASK -- COMMAND...
loftd --waypipe exec TASK -- GUI_APP
loftd --waypipe=/path/to/client.sock exec TASK -- GUI_APP
```

- Ordinary exec does not interact with Waypipe.
- Valueless Waypipe exec requires a Waypipe-capable task and reuses its running server.
- Valued Waypipe exec requires a Waypipe-capable task, replaces its target and server, and starts the command only after the new display is ready.
- Tasks whose active records lack Waypipe metadata remain usable for `ps`, `attach`, `kill`, and supported ordinary exec, but Waypipe exec reports that relaunch is required.

## Host architecture

The task supervisor owns:

- one task-private Unix data listener mapped to the guest Waypipe vsock port;
- one task-private Unix control listener used by valued exec requests;
- the current external Unix socket target;
- serialization of valued target replacements;
- cleanup of both task-private socket paths when the task ends.

A guest data connection waits while no target is active. Once a target is selected, the broker connects to it and byte-proxies the stream. Waypipe protocol data remains separate from loftd control messages.

A valued exec opens the control socket, supplies the new target, and keeps that control connection open for the duration of the exec request. This serializes target replacement and command startup. The target becomes available before guest server restart, so the new server can establish its transport while the control connection remains held.

## Guest architecture

Guest-init owns a serialized `WaypipeService` containing the current child process and stable display path.

Startup:

1. Remove a stale `loftd-waypipe-0` display socket.
2. Start the Rust Waypipe server using the task's fixed vsock port.
3. Wait for the stable display socket.
4. Export the shared Waypipe environment for the primary and exec commands.
5. Monitor unexpected server exit as a fatal loss of a declared VM-level capability.

Valueless exec calls `reuse`:

- verify that the child is still running;
- verify that the stable display socket exists;
- leave the process and connected GUI applications untouched.

Valued exec calls `replace` while holding the service lock:

1. Take the current child from managed state.
2. Terminate and reap it if it is still running.
3. Remove the old display socket.
4. Start a fresh Waypipe server with the same display name and vsock port.
5. Wait for display readiness.
6. Store the new child as the current service.
7. Start the requested command through the unchanged non-PTY exec stream handling.

If replacement fails, that exec request fails and the primary task remains alive. The service remains explicitly without a current child rather than misclassifying the intentional stop as an unexpected server exit. A later valued exec may attempt replacement again.

## Lifecycle and failure semantics

- The primary managed PTY remains the task and VM lifetime owner.
- Primary exit cleans up active exec process groups, the current Waypipe server, host brokers, and task-private sockets.
- Ordinary exec startup/runtime failure affects only that request.
- Valueless Waypipe exec fails if the current server or display is unavailable.
- Valued replacement failure affects only that request; ordinary exec and attach remain available.
- Unexpected exit of a current managed Waypipe server remains fatal for a Waypipe-enabled task.
- External transport silence is not treated as disconnection. EOF, hangup, or terminal socket errors determine transport loss.
- Replacing a target does not preserve existing GUI applications.

## Compatibility

The launch config records Waypipe capability independently from an initial target. Its guest contract includes the fixed Waypipe port whenever the task has the capability.

The active-task record contains optional Waypipe control-socket metadata. Missing metadata parses as no Waypipe capability, preserving older records for existing task-control operations.

The exec protocol version is incremented because the start frame now carries one of three Waypipe actions:

- `Disabled`
- `Reuse`
- `Replace`

A host refuses exec against a task using a different protocol version instead of allowing either endpoint to misinterpret the frame.

## Security

- External Waypipe targets must be absolute existing Unix socket paths.
- Loftd does not expose an unauthenticated raw TCP Waypipe endpoint.
- Loftd does not start or manage SSH or the workstation Waypipe client.
- Task-private listeners use the existing loftd runtime-directory ownership and path-budget rules.
- Guest exec commands continue to run as user `dev` under the existing exec security policy.

## Testing and validation

Host tests cover:

- valueless launch and exec parsing;
- valued launch and replacement exec parsing;
- launch planning with capability but no initial target;
- launch-config and active-record compatibility;
- waiting for initial target activation;
- target activation and stream proxying;
- Waypipe capability rejection for legacy tasks;
- exec protocol action round trips and version mismatch handling.

Guest tests cover:

- startup and readiness of the stable display;
- valueless reuse checks;
- valued stop, reap, restart, and readiness ordering;
- replacement failure isolation;
- command startup only after Waypipe action success;
- shared environment for primary and exec commands.

Validation uses:

```bash
nix develop --command cargo fmt --check
nix develop --command cargo clippy --all-targets --all-features -- -D warnings
nix develop --command cargo deny check
nix develop --command cargo test
```

If inherited Cargo configuration redirects crates.io to a missing `/vendor`, use the repository's Nix-managed build and report direct Cargo-based checks as environment-blocked.
