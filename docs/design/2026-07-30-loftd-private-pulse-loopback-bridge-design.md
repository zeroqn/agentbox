# Loftd Private Host-Loopback Pulse Bridge Design

## Status

Approved on 2026-07-30.

## Problem

Loftd currently treats `--pulse=tcp:IP:PORT` as a literal PulseAudio-compatible TCP endpoint and exports it to the guest as `PULSE_SERVER`. This works when the endpoint is reachable through the guest network, but `tcp:127.0.0.1:PORT` points at the guest loopback interface rather than a host service bound to host loopback.

Passt's host-loopback mapping is not suitable because it would expose the host loopback address broadly instead of granting access only to the selected Pulse port. The bridge must expose exactly one configured host endpoint without changing the task's general networking behavior.

## Goals

- Support host Pulse-compatible listeners bound only to `127.0.0.1:PORT`.
- Interpret both `tcp:localhost:PORT` and `tcp:127.0.0.1:PORT` as explicit requests for a private host-loopback bridge.
- Expose only the selected host loopback port to the task.
- Work independently of passt and TSI network modes.
- Preserve existing direct guest TCP behavior for non-loopback literal IP endpoints.
- Allow task startup when the host Pulse listener is temporarily unavailable.
- Support multiple simultaneous Pulse client connections.

## Non-goals

- Passt `--map-host-loopback` support.
- General host-loopback access from the guest.
- Arbitrary guest-selected proxy destinations.
- Native PipeWire protocol or `pipewire-0` socket forwarding.
- Pulse cookie forwarding or authentication setup.
- Pulse protocol filtering or authorization narrowing.
- IPv6 host-loopback bridging in the first version.
- Preflight connection checks during task launch.
- A framed or multiplexed bridge protocol.

## CLI contract

`--pulse` continues to use a `tcp:` endpoint, but the parsed endpoint becomes typed:

- `--pulse=tcp:localhost:4714` selects a private bridge to host `127.0.0.1:4714`.
- `--pulse=tcp:127.0.0.1:4714` selects the same private bridge.
- `--pulse=tcp:192.0.2.10:4714` remains a direct guest TCP endpoint.
- `--pulse=tcp:[2001:db8::1]:4714` remains a direct guest TCP endpoint.
- `--pulse=tcp:[::1]:4714` remains direct guest TCP in the first version.
- Hostnames other than the exact `localhost` spelling remain invalid.
- Port zero remains invalid.

The direct endpoint form remains literal. Loftd does not resolve arbitrary hostnames or rewrite non-loopback addresses.

## Architecture

The host-loopback form uses one private bridge per task:

```text
guest Pulse client
PULSE_SERVER=unix:/run/user/<uid>/loftd-pulse
        |
        v
guest Unix listener
        | one AF_VSOCK connection per Pulse connection
        v
dedicated task vsock port
        | krun_add_vsock_port2
        v
private host Unix socket under /tmp/loftd-<uid>/
        | fixed destination, selected at task launch
        v
host TCP connection to 127.0.0.1:<port>
```

The host Unix socket is required by libkrun's existing vsock port mapping API. It is an internal endpoint, not a guest mount or network listener. A direct guest-vsock-to-host-TCP mapping would require a new libkrun API and is outside this change.

Each Pulse connection uses an independent byte stream from the guest Unix client through vsock and the private host Unix endpoint to one host TCP connection. The bridge does not add framing, multiplexing, destination negotiation, or application-protocol awareness.

## Endpoint model

The host CLI model distinguishes:

- `HostLoopback { port }`
- `Direct { address: SocketAddr }`

Only `HostLoopback` produces bridge configuration. `Direct` continues to produce the canonical literal `PULSE_SERVER=tcp:IP:PORT` value.

The serialized helper launch configuration contains an optional Pulse bridge record with:

- The private host Unix socket path.
- The dedicated guest vsock port.
- The fixed host TCP port.

The guest environment contract distinguishes direct Pulse configuration from bridged Pulse configuration. The fixed host destination remains in host-side configuration; guest-init receives only the vsock port needed to reach the bridge.

## Host components

### Socket allocation

The bridge uses the existing owner-only per-user loftd runtime directory under `/tmp/loftd-<uid>/`. Socket allocation follows the same path-length, stale-socket, ownership, and mode checks used by managed attach, exec, and Waypipe sockets.

The Pulse bridge receives a distinct randomized per-task socket path and a dedicated guest vsock port. It does not reuse attach, exec, or Waypipe ports.

### Pulse bridge lifecycle

A focused host-side `PulseBridge` component starts before VM entry:

- Bind the private Unix listener.
- Accept libkrun-proxied connections.
- For every accepted connection, start an independent relay operation.
- Connect that operation only to `127.0.0.1:<configured-port>`.
- Copy bytes bidirectionally until either side terminates.
- Close only the affected connection on connect or relay failure.
- Continue accepting later connections.

The bridge is owned by the task supervisor. Dropping it at task shutdown stops new accepts, terminates active bridge resources, and removes the private Unix socket.

### Libkrun mapping

The direct libkrun launcher registers the Pulse bridge using `krun_add_vsock_port2` with the direction in which a guest AF_VSOCK connection is connected to the host Unix endpoint. This follows the existing Waypipe data transport direction rather than managed attach's host-connects-to-guest-listener direction.

Failure to register the mapping fails task launch.

## Guest components

When bridged Pulse is configured, guest-init starts a focused Pulse bridge component before launching the interactive or managed workload:

- Ensure the development user's runtime directory exists.
- Remove only a stale socket at the fixed guest Pulse path.
- Bind `/run/user/<uid>/loftd-pulse` as a Unix stream listener.
- Set ownership and permissions for the development user.
- Accept Pulse client connections.
- For each accepted Unix connection, open one AF_VSOCK stream to the configured dedicated port.
- Copy bytes bidirectionally until either side terminates.
- Continue accepting later connections after per-connection failures.

Guest-init exports:

```bash
PULSE_SERVER=unix:/run/user/<uid>/loftd-pulse
```

Direct endpoints continue to export:

```bash
PULSE_SERVER=tcp:192.0.2.10:4714
```

The guest listener must be ready before the initial command and managed exec service can launch workloads that inherit the environment.

## Data flow and concurrency

For every Pulse client connection:

- The guest Pulse client connects to the guest Unix socket.
- Guest-init opens a new AF_VSOCK stream to the task's Pulse bridge port.
- Libkrun connects that stream to the private host Unix listener.
- The host bridge opens a new TCP connection to the fixed host loopback target.
- The guest and host relays copy bytes in both directions.
- Connection shutdown affects only this stream.

No connection is retained for reuse. This matches Pulse's stream semantics and avoids a custom multiplexing protocol.

## Failure behavior

- Invalid CLI endpoint fails before task launch.
- Failure to allocate or bind the private host Unix socket fails task launch.
- Failure to register the libkrun mapping fails task launch.
- Failure to create the guest Pulse Unix listener fails guest bootstrap.
- If `127.0.0.1:PORT` is unavailable, task launch still succeeds.
- A client attempt while the host listener is unavailable fails that connection only.
- The bridge continues accepting later attempts so a restarted host Pulse service can become usable without restarting the task.
- Relay errors are reported through existing host or guest diagnostic channels without terminating unrelated connections or the task.
- Task shutdown removes the host socket through supervisor cleanup. Guest socket lifetime is bounded by guest-init and VM lifetime.

## Security

The bridge grants access only to the configured host TCP endpoint:

- No passt host-loopback mapping is enabled.
- No host TCP listener is created.
- No guest-visible host-loopback address is introduced.
- The guest cannot select a destination through bridge traffic.
- The host target is fixed as `127.0.0.1:<launch-time-port>`.
- The private host Unix socket is stored in the existing owner-only `0700` runtime directory.
- The private host socket is not mounted into the guest and is not remotely reachable.
- Socket and bridge lifetime are scoped to one task.

The bridge narrows network reachability, not Pulse protocol capabilities. Depending on host Pulse-compatible server policy, a connected client may obtain playback, capture, stream inspection, or server-control access. Host Pulse configuration remains responsible for protocol authorization.

## Compatibility

The launch-config codec must preserve its existing version and compatibility rules. The new optional bridge fields must decode safely when absent so existing direct endpoints and older serialized configurations continue to work according to the repository's established codec policy.

Existing tasks retain the endpoint selected at launch. `loftd exec` inherits either the direct endpoint or bridge configured for that task and cannot retarget it.

## Expected implementation areas

Host crate:

- `crates/loftd/src/cli/mod.rs`
- `crates/loftd/src/runtime/launch/config/model.rs`
- `crates/loftd/src/runtime/launch/config/codec.rs`
- `crates/loftd/src/runtime/launch/config/guest_env.rs`
- `crates/loftd/src/runtime/launch/plan.rs`
- `crates/loftd/src/runtime/session/managed_attach_socket.rs`
- `crates/loftd/src/runtime/session/supervisor/entry.rs`
- `crates/loftd/src/runtime/vm/libkrun/launcher.rs`
- A focused Pulse bridge module under `crates/loftd/src/runtime/session/`

Guest crate:

- Guest launch-contract parsing under `crates/loftd-guest-init/src/guest_init/runtime/`
- Shell environment construction under `crates/loftd-guest-init/src/guest_init/components/shell/`
- A focused Pulse bridge component under `crates/loftd-guest-init/src/guest_init/components/`

Documentation:

- `README.md`

## Testing

### Host tests

- Parse and canonicalize `localhost` bridge syntax.
- Parse IPv4 loopback as bridge syntax.
- Preserve direct IPv4 and IPv6 endpoint behavior.
- Keep IPv6 loopback direct in the first version.
- Reject unsupported hostnames and port zero.
- Round-trip the optional bridge launch configuration through the codec.
- Decode configurations without bridge fields according to codec compatibility rules.
- Verify the dedicated libkrun mapping port, path, and direction.
- Verify bidirectional relay transfer.
- Verify multiple independent connections.
- Verify the target is always the configured host loopback port.
- Verify connection refusal does not terminate the bridge listener.
- Verify private socket cleanup.

### Guest tests

- Create the Pulse Unix socket with the expected path, ownership, and permissions.
- Open one vsock connection for each Unix client.
- Verify bidirectional transfer through a controllable transport seam.
- Verify a failed connection does not terminate the listener.
- Export the Unix `PULSE_SERVER` value for bridged endpoints.
- Preserve direct `PULSE_SERVER` values for non-loopback endpoints.
- Start the listener before command execution.

### Repository validation

```bash
nix develop --command cargo fmt --check
nix develop --command cargo clippy --all-targets --all-features -- -D warnings
nix develop --command cargo deny check
nix develop --command cargo test
```

### Live validation

Run a host Pulse-compatible listener bound only to `127.0.0.1:4714`, then launch loftd with `--pulse=tcp:localhost:4714` and verify guest audio playback. Confirm that:

- The guest receives a Unix `PULSE_SERVER` value.
- Playback reaches the host listener.
- Passt host-loopback mapping is absent.
- Other host-loopback ports remain inaccessible from the guest.
- Stopping and restarting the host Pulse listener allows a later guest connection without relaunching the task.
- Combined Waypipe playback uses mpv's required software-renderer opt-in, such as `mpv --gpu-sw=yes`.
