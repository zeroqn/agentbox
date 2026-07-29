# loftd Pulse TCP passthrough design

Date: 2026-07-29
Status: Approved

## Summary

Add an opt-in `loftd --pulse=tcp:IP:PORT` launch option that directs PulseAudio-compatible applications inside a new loftd guest to a host-provided Pulse TCP server.

The first version uses a direct client connection. Loftd does not proxy the Pulse protocol, create a guest-local Pulse Unix socket, or start PipeWire services inside the guest. The host is responsible for running PipeWire with `pipewire-pulse`, exposing an authenticated or ACL-restricted TCP listener, and making the supplied address reachable from the selected loftd network mode.

## Goals

- Accept one optional `--pulse=tcp:IP:PORT` argument for new task launches.
- Validate and normalize the endpoint before VM startup.
- Make the endpoint available to guest PulseAudio-compatible clients as `PULSE_SERVER=tcp:IP:PORT`.
- Support the default passt network and opt-in TSI network without adding a separate audio transport.
- Document the required host `pipewire-pulse` TCP listener setup and its security implications.
- Preserve existing behavior when `--pulse` is omitted.

## Non-goals

- Running `pipewire`, `pipewire-pulse`, or a PulseAudio daemon inside the guest.
- Exposing a guest-local `$XDG_RUNTIME_DIR/pulse/native` socket.
- Implementing a host or guest Pulse protocol proxy.
- Forwarding the native PipeWire `pipewire-0` protocol.
- Supporting native PipeWire clients that cannot use PulseAudio compatibility.
- Forwarding Pulse cookies or other authentication secrets into the guest.
- Configuring the host firewall, PipeWire daemon, access control, or listener lifecycle.
- Probing the endpoint or requiring it to be reachable before VM startup.
- Changing audio settings for already-running tasks through `loftd exec` or `loftd attach`.

## User interface

A new top-level launch option is added:

```text
--pulse=tcp:IP:PORT
```

Examples:

```bash
loftd --pulse=tcp:192.168.1.10:4713
loftd --tsi --pulse=tcp:127.0.0.1:4713
loftd --pulse=tcp:[2001:db8::10]:4713 -- paplay sample.wav
```

The parser accepts only the `tcp:` scheme followed by a valid socket address. IPv6 addresses use brackets. Ports must be in the valid nonzero TCP port range. Hostnames, Unix socket paths, omitted schemes, missing addresses, and malformed ports are rejected by CLI parsing.

The supplied address is used literally. Loftd does not rewrite loopback addresses, discover a host address, or select a PipeWire listener automatically.

The option applies only when launching a new task. Management subcommands keep their existing behavior.

## Architecture

### Host CLI and runtime model

The host CLI parses the value into a dedicated Pulse TCP endpoint type rather than retaining an unvalidated string. The type stores a socket address and formats the canonical Pulse server value as `tcp:IP:PORT`, including IPv6 brackets where required.

The optional endpoint is carried through the existing launch flow:

- CLI launch options
- `RuntimeOptions`
- `LaunchPlan`
- `LaunchSpec`
- serialized launch configuration
- guest configuration environment

The launch configuration exposes the endpoint to guest-init through an internal `LOFTD_PULSE_SERVER` value. The host does not broadly copy `PULSE_SERVER` from its own environment.

Older serialized launch configurations that omit the new field remain valid and mean that Pulse passthrough is disabled. Newly serialized configurations explicitly preserve the optional endpoint.

### Guest initialization

Guest-init reads the optional `LOFTD_PULSE_SERVER` contract during normal loftd entry setup. When present, it validates the same `tcp:IP:PORT` shape and adds the canonical value to the final guest environment as:

```text
PULSE_SERVER=tcp:IP:PORT
```

The environment is established before the managed shell or direct guest command starts, so child processes inherit it in both launch paths.

Guest-init does not start or supervise an audio daemon. It does not create runtime directories or Pulse socket paths beyond the existing general guest setup.

### Host PipeWire setup

The host must already run PipeWire's PulseAudio compatibility server and expose a TCP listener. A typical user-level drop-in is placed under:

```text
~/.config/pipewire/pipewire-pulse.conf.d/loftd-tcp.conf
```

A representative configuration is:

```ini
pulse.properties = {
    server.address = [
        "unix:native"
        "tcp:127.0.0.1:4713"
    ]
}
```

The exact listen address must match the selected network topology. The user restarts the host `pipewire-pulse` user service after changing its configuration.

Loftd documentation will make clear that a TCP Pulse listener can expose playback, capture, and other Pulse server operations to connecting clients. The host configuration must restrict access using PipeWire-Pulse access controls, network isolation, or another host-managed policy. Loftd does not enable anonymous access itself.

## Network behavior

### TSI

With `--tsi`, guest outbound TCP connections use the host network context. A host loopback listener such as `tcp:127.0.0.1:4713` is therefore the expected setup when the local libkrun TSI implementation permits that connection.

The feature relies on the existing TSI implementation and does not add a reserved vsock mapping.

### Default passt

With the default passt network, the endpoint must be reachable from the guest's virtual network. A host listener bound only to host loopback may not be reachable through passt. The user must provide an address and listener binding appropriate for the host and passt topology.

Loftd performs syntax validation only. Connection failures remain normal Pulse client errors inside the guest.

## Data flow

- The user starts `loftd --pulse=tcp:IP:PORT`.
- Clap invokes the dedicated endpoint parser.
- Loftd stores the validated endpoint in the runtime and launch models.
- Launch configuration serialization preserves the optional endpoint.
- Launch configuration construction emits `LOFTD_PULSE_SERVER=tcp:IP:PORT` for guest-init.
- Guest-init validates the internal value and exports `PULSE_SERVER=tcp:IP:PORT`.
- A PulseAudio-compatible guest application opens a normal TCP connection through passt or TSI.
- The host `pipewire-pulse` server accepts or rejects that connection according to its host-side policy.

## Error handling

Loftd fails before VM startup when:

- the option does not begin with `tcp:`;
- the address is missing or malformed;
- an IPv6 address is not bracketed;
- the port is missing, zero, or outside the valid range.

Guest-init fails closed if the serialized internal Pulse endpoint is malformed. This protects the guest boundary from corrupted or manually edited launch configuration.

Loftd does not fail launch because:

- the host service is not running;
- the endpoint refuses the connection;
- the endpoint is unreachable from the selected network mode;
- host authentication rejects a client.

Those conditions are reported by the guest Pulse client when it attempts to connect.

## Security

The feature gives guest workloads access to whichever Pulse server is named by the user. Depending on host policy, that can permit audio playback, microphone capture, stream inspection, or server control.

The design therefore keeps the capability explicit and disabled by default. Loftd does not automatically expose host PipeWire on a wildcard address, alter firewall rules, enable anonymous authentication, or forward credentials.

A loopback-only listener with TSI is the recommended local configuration when it works for the host's libkrun setup. For passt, users must deliberately select a guest-reachable address and restrict it outside loftd.

## Testing

### Host crate

- CLI parsing accepts valid IPv4 and bracketed IPv6 TCP endpoints.
- CLI parsing rejects unsupported schemes, hostnames, missing addresses, malformed IPv6, zero ports, and invalid ports.
- Omitted `--pulse` leaves the option disabled.
- `RuntimeOptions`, `LaunchPlan`, and `LaunchSpec` preserve the endpoint.
- Launch configuration codec round-trips the endpoint.
- Legacy launch configuration without the new value decodes with Pulse disabled.
- Guest configuration emits the exact canonical `LOFTD_PULSE_SERVER` value only when enabled.
- Existing passt and TSI launch planning remains otherwise unchanged.

### Guest crate

- A valid internal endpoint produces the exact `PULSE_SERVER` environment value.
- No endpoint leaves `PULSE_SERVER` unset by this feature.
- Malformed internal values are rejected.
- Managed-session and direct-command paths inherit the same value.

### Repository and documentation checks

- README documents host `pipewire-pulse` setup, passt versus TSI reachability, security ownership, and example guest commands.
- No PipeWire or Pulse daemon package is added to the guest image solely for this feature.

### Manual validation

- Configure a host `pipewire-pulse` TCP listener.
- Launch a current loftd and matching guest-init with `--pulse`.
- Confirm `PULSE_SERVER` inside the guest has the canonical value.
- Use a Pulse-compatible diagnostic or playback client to confirm connection and playback.
- If host capture is authorized, verify a Pulse-compatible recording client separately.
- Exercise both TSI loopback and a guest-reachable passt endpoint where the environment supports them.

## Implementation boundaries

Expected implementation areas are:

- `crates/loftd/src/cli/mod.rs`
- loftd launch plan and launch configuration model/codec code under `crates/loftd/src/runtime/launch/`
- `crates/loftd-guest-init/src/guest_init/runtime/loftd.rs` and focused environment tests
- `README.md`

The Nix image package list should remain unchanged unless implementation-time testing proves that an existing image lacks the Pulse-compatible client tooling required by an already documented smoke test. Such tooling is not required for the runtime feature itself.
