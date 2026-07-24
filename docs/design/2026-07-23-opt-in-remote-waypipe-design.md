# Opt-in loftd remote Waypipe design

## Summary

Add an opt-in loftd launch mode for running one GUI application inside a new loftd guest VM and displaying it through a workstation-side Waypipe client.

The interface is:

```bash
loftd \
  --workspace=/home/dev/foo \
  --waypipe=/tmp/loftd-waypipe-xxx.sock \
  -- gui-application
```

The standalone `--workspace` option selects the host workspace directory and defaults to the current working directory when omitted. `--waypipe` identifies an existing Unix socket on the loftd host that is owned by SSH reverse forwarding. loftd bridges that socket to a dedicated guest vsock port and launches the requested command under a guest Waypipe server.

This is separate from loftd's existing `--wayland` local compositor passthrough.

## Goals

- Launch a new loftd VM using an explicit host workspace directory.
- Mount that directory at `/workspace` in the guest.
- Connect a guest Waypipe server to an existing SSH-forwarded Unix socket on the loftd host.
- Run the requested guest command under Waypipe.
- Keep SSH authentication, encryption, and socket ownership outside loftd.
- Start with software-only Waypipe forwarding through `--no-gpu`.
- Produce clear validation errors before VM startup where possible.

## Non-goals

- Connecting Waypipe to an already-running loftd task.
- Automatically starting SSH or the workstation Waypipe client.
- Creating, listening on, unlinking, or cleaning up the host Unix socket.
- Replacing or extending the existing `--wayland` cross-domain passthrough.
- Supporting `--waypipe` and `--wayland` in the same launch.
- Forwarding dmabufs or enabling Waypipe GPU acceleration.
- Forwarding GUI applications that were not launched under the Waypipe server.
- Providing persistent or reconnectable GUI sessions through a nested compositor.
- Exposing an unauthenticated TCP Waypipe endpoint.

## User workflow

### Workstation

Start a Waypipe client connected to the workstation compositor:

```bash
waypipe \
  --socket "$XDG_RUNTIME_DIR/loftd-waypipe.sock" \
  client
```

Create an SSH reverse Unix-socket forward to the loftd host:

```bash
ssh \
  -R /tmp/loftd-waypipe-xxx.sock:"$XDG_RUNTIME_DIR/loftd-waypipe.sock" \
  loftd-host
```

### loftd host

Launch a new guest and run the GUI application:

```bash
loftd \
  --workspace=/home/dev/foo \
  --waypipe=/tmp/loftd-waypipe-xxx.sock \
  -- gui-application
```

## CLI contract

`--workspace=WORKSPACE` and `--waypipe=SOCKET` are optional top-level launch arguments.

- `WORKSPACE` is an absolute host directory.
- `WORKSPACE` becomes the launch working directory and is mounted at guest `/workspace`.
- If `--workspace` is omitted, loftd uses the current working directory.
- `SOCKET` is an absolute host Unix-socket path.
- `SOCKET` must already exist and be a Unix socket before loftd starts the VM.
- The socket is owned by the SSH reverse-forwarding process.
- loftd connects to the socket but does not create, unlink, or remove it.
- A guest command is required for the initial version.
- `--waypipe` is mutually exclusive with `--wayland`.
- The initial version uses Waypipe `--no-gpu` and does not require `--gpu=drm`.

The argument formats are intentionally limited to one absolute path each. More Waypipe tuning options are excluded until a concrete use case requires them.

## Architecture

### Host CLI and launch planning

The loftd host parses `--workspace=WORKSPACE` and `--waypipe=SOCKET` into independent optional paths:

- selected host workspace path
- host Waypipe socket path

Launch startup selects `--workspace` or the current working directory, canonicalizes it, and verifies it is a directory. Existing workspace naming, state placement, and `/workspace` mount behavior use that selected directory as their source.

The launch plan carries a Waypipe configuration independently from the existing Wayland and GPU configuration.

### Launch contract

The serialized host-to-helper launch configuration gains an optional Waypipe channel containing:

- host Unix-socket path
- dedicated guest vsock port

The configuration is optional so ordinary launches preserve their current behavior.

The launch-contract codec must follow the repository's existing compatibility rules for optional fields and reject malformed partial configuration.

### libkrun transport

The direct libkrun launcher registers a second vsock mapping when Waypipe is enabled.

The mapping is independent of managed attach:

- managed attach keeps its existing guest port and host socket
- Waypipe receives its own fixed or centrally allocated guest port
- the mapping direction allows a guest Waypipe server to initiate a connection through vsock
- libkrun connects that stream to the existing host Unix socket

The exact `krun_add_vsock_port2` boolean must follow the verified libkrun direction semantics rather than being inferred from the parameter name.

### Guest launch

Guest-init detects the optional Waypipe launch configuration and wraps the requested command with the guest Waypipe server. The effective command is equivalent to:

```bash
waypipe \
  --no-gpu \
  --vsock \
  --socket PORT \
  server \
  -- gui-application
```

The guest side omits a CID because it connects from the guest to the host. Guest identity, working directory, environment setup, and command execution should otherwise follow the normal loftd workload path.

Waypipe must be available in the loftd guest image and guest `PATH`. It should not be added to the agentbox image.

## Data flow

```text
workstation Wayland compositor
  ↑
workstation Waypipe client
  ↑
workstation Unix socket
  ↑ SSH reverse Unix-socket forwarding
host /tmp/loftd-waypipe-xxx.sock
  ↑ libkrun Unix-socket connector
host/guest vsock mapping
  ↑
guest Waypipe server --no-gpu
  ↑
guest GUI application
```

The SSH connection remains the security boundary for remote transport. loftd does not expose Waypipe directly over TCP.

## Interaction with existing features

### Existing `--wayland`

`--waypipe` and `--wayland` are mutually exclusive.

- `--wayland` forwards to a compositor local to the loftd host through the cross-domain proxy and virtio-gpu integration.
- `--waypipe` forwards a newly launched application to a remote workstation through Waypipe, vsock, a host Unix socket, and SSH.

They solve different transport problems and should remain separate.

### GPU mode

The initial Waypipe mode always uses `--no-gpu`.

It does not automatically select `--gpu=drm`. If an explicitly supplied GPU mode conflicts with the software-only contract, loftd should reject it rather than silently changing its meaning.

### Managed sessions

The Waypipe channel is independent of the managed attach channel. If existing CLI rules permit a managed launch with a guest command, both vsock mappings may coexist, each with its own guest port and host socket.

The initial Waypipe design does not add GUI reconnection semantics. Reattaching the terminal does not recreate a closed Waypipe connection or reconnect an application to a new compositor proxy.

## Validation and errors

Before starting the VM, loftd validates:

- `WORKSPACE` is absolute
- `WORKSPACE` exists
- `WORKSPACE` is a directory
- `SOCKET` is absolute
- `SOCKET` exists
- `SOCKET` is a Unix socket
- a guest command was supplied
- `--waypipe` is not combined with `--wayland`
- the selected GPU mode is compatible with software-only Waypipe

Representative errors include:

```text
workspace must be an absolute path: foo
failed to canonicalize loftd workspace mount
loftd workspace is not a directory: /home/dev/foo
waypipe socket must be an absolute path: loftd-waypipe.sock
waypipe socket does not exist: /tmp/loftd-waypipe-xxx.sock
waypipe transport is not a Unix socket: /tmp/loftd-waypipe-xxx.sock
--waypipe requires a guest command
--waypipe cannot be combined with --wayland
--waypipe software transport cannot be combined with --gpu=drm
```

Runtime errors should identify the failing boundary:

- libkrun Waypipe mapping registration
- connection to the host Unix socket
- missing guest Waypipe executable
- guest Waypipe server startup
- launched GUI command failure

The host socket may disappear after validation because SSH exits. This is a normal runtime race; the resulting connection failure should be reported without loftd attempting to recreate or replace the socket.

## Lifecycle

- SSH owns the host socket and removes it according to SSH forwarding behavior.
- loftd does not unlink the socket before or after launch.
- The guest Waypipe server lifetime is tied to the launched GUI command.
- The VM lifetime follows the normal loftd command lifecycle.
- Closing the workstation Waypipe client or SSH transport causes the guest Waypipe connection or command to fail according to Waypipe behavior.
- No persistent background Waypipe service is introduced in the first version.

## Testing considerations

### Host CLI tests

- Parse valid standalone `--workspace=WORKSPACE` and `--waypipe=SOCKET` values, together and independently.
- Reject empty or relative workspace and socket paths.
- Reject `--waypipe` with `--wayland`.
- Reject `--waypipe` with `--gpu=drm`.
- Require a guest command when `--waypipe` is present.

### Launch planning tests

- Use the selected workspace, or current working directory when omitted, as the source mounted at `/workspace`.
- Validate workspace and socket types.
- Preserve ordinary launch behavior when Waypipe is absent, including standalone workspace selection.
- Carry the Waypipe channel into the launch configuration.

### Launch-contract tests

- Round-trip an optional Waypipe channel.
- Preserve compatibility for configurations without Waypipe.
- Reject incomplete or malformed Waypipe fields.

### libkrun tests

- Register the Waypipe vsock mapping with the correct port, socket path, and direction.
- Allow managed attach and Waypipe mappings to coexist without port collision.
- Report mapping registration failures with Waypipe-specific context.
- Do not register a Waypipe mapping for ordinary launches.

### Guest-init tests

- Wrap the guest command with `waypipe --no-gpu --vsock` when enabled.
- Preserve the normal command path when disabled.
- Preserve guest identity, environment, and `/workspace` working directory.
- Propagate Waypipe or application exit status according to the normal workload contract.

### Nix image checks

- Verify Waypipe exists only in the loftd image variant.
- Verify the guest binary is available in `PATH`.

### Live smoke test

Use a real workstation compositor, SSH reverse Unix-socket forwarding, a built loftd host binary, and the matching current guest-init/image. Launch a simple Wayland application and verify:

- the host SSH-forwarded socket exists before launch
- libkrun registers the guest-to-host Waypipe mapping
- the guest Waypipe server connects
- the application window appears on the workstation compositor
- application exit terminates the Waypipe server and VM normally
- disconnecting SSH produces a clear runtime failure

## Future extensions

Possible later work, excluded from this design:

- a one-command workstation wrapper using `waypipe ssh --remote-bin`
- attaching a GUI command to an existing loftd task
- optional Waypipe compression settings
- GPU/dmabuf forwarding
- X11 applications through Waypipe `--xwls`
- persistent nested-compositor sessions

Each extension should preserve SSH as the authenticated remote boundary and keep local `--wayland` passthrough separate from remote Waypipe transport.
