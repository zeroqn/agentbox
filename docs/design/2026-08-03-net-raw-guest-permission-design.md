# `net-raw` guest permission design

## Goal

Expose an explicit `net-raw` value in loftd's existing `--permissions` option. A requesting guest `dev` workload receives Linux `CAP_NET_RAW` without changing the behavior of other permissions.

## Scope

Included:

- Host CLI parsing, help text, launch configuration, and typed permission model support for `net-raw`.
- Guest-init parsing and capability planning support for `net-raw`.
- Delivery of capability number 13 (`CAP_NET_RAW`) through the existing effective, permitted, inheritable, and ambient capability flow.
- Focused tests for parsing, serialization, and capability selection.

Excluded:

- Adding networking tools to the guest image.
- Changing nftables, TPROXY, or routing configuration.
- Making `net-admin` imply `net-raw`.
- Changing privileges for root workloads.

## Interface

The public syntax is:

```text
loftd --permissions=net-raw
loftd --permissions=net-admin,net-raw
```

`net-raw` remains opt-in. Launches without it do not receive `CAP_NET_RAW`.

## Design

The host and guest maintain matching typed `GuestPermission` enums. Add a `NetRaw` variant to both sets, render and parse it as `net-raw`, and include it in allowed-value diagnostics and CLI help.

The existing launch configuration serializes nonempty permissions into `LOFTD_PERMISSIONS`, so no new contract field is required. Guest-init parses that environment value and adds capability 13 to the workload capability plan only when `NetRaw` is selected.

The existing credential transition already applies its planned capabilities to the `dev` user via the bounding set, effective/permitted/inheritable sets, and ambient set. `CAP_NET_RAW` therefore follows the same path for initial commands, interactive shells, and later guest workload execution.

## Tests and validation

- Update host permission parser/model tests to accept and serialize `net-raw`, including composition with `net-admin`.
- Update guest environment parsing tests.
- Update capability-plan tests to prove `net-raw` maps to capability 13.
- Run targeted tests, then formatting, Clippy with warnings denied, and workspace tests.
