# Unified loftd guest permissions design

## Summary

Replace the top-level `--io-uring` and `--perf` flags with one comma-separated option:

```bash
loftd --permissions=io-uring,net-admin,bpf,perf
```

The option selects explicit per-launch guest permissions. `io-uring` and `perf` preserve their existing guest-kernel policy behavior. `net-admin` and `bpf` grant the guest `dev` workload `CAP_NET_ADMIN` and `CAP_BPF`, respectively.

No permission is enabled by default. The selected Linux capabilities apply to all guest `dev` workload paths for the task, including the initial command, managed PTY command, hidden `as-dev` transition, and later `loftd exec` commands. They exist only inside the guest VM and do not grant capabilities to host processes or host namespaces.

This is an immediate CLI replacement. The removed `--io-uring` and `--perf` flags do not remain as aliases.

## Goals

- Provide one explicit interface for optional guest permissions.
- Preserve the existing secure defaults for io_uring and performance observability.
- Allow guest `dev` workloads to opt into `CAP_NET_ADMIN` for guest networking administration, including nftables and TPROXY setup.
- Allow guest `dev` workloads to opt into `CAP_BPF` for privileged BPF operations.
- Apply selected capabilities consistently to the initial workload and later commands in the same managed task.
- Keep all authority scoped to the guest VM.
- Fail guest initialization rather than silently omit a requested permission.

## Non-goals

- Grant host `CAP_NET_ADMIN`, `CAP_BPF`, or other host privileges.
- Add `CAP_PERFMON` or arbitrary Linux capability names. `CAP_SYS_ADMIN` is available only
  through the explicit independent `sys-admin` permission and `loftd-granted`.
- Make `bpf` imply `net-admin`, `perf`, `sys-admin`, or any other permission.
- Change nested Podman capability or seccomp defaults.
- Automatically grant selected guest capabilities to processes inside nested containers.
- Install BPF development tools, loaders, programs, nftables, or TPROXY tooling in the guest image.
- Add a launch-time BPF or networking connectivity preflight.
- Guarantee that every BPF program type can load with `CAP_BPF` alone.
- Keep compatibility aliases for `--io-uring` or `--perf`.

## CLI contract

Add a top-level option accepting a comma-separated permission list:

```text
--permissions=PERMISSION[,PERMISSION...]
```

Supported values are:

- `io-uring`
- `net-admin`
- `net-raw`
- `bpf`
- `perf`
- `sys-admin`

Examples:

```bash
loftd --permissions=io-uring
loftd --permissions=net-admin,bpf
loftd --permissions=io-uring,net-admin,bpf,perf
```

Parsing rules:

- Values are case-sensitive and use the exact spellings above.
- Input order does not affect behavior.
- Duplicate values are accepted and deduplicated.
- Unknown values fail CLI parsing.
- Empty entries fail CLI parsing.
- An explicitly empty `--permissions=` value fails CLI parsing.
- Omitting the option selects an empty permission set.
- Serialization uses the stable canonical order `io-uring,net-admin,net-raw,bpf,perf,sys-admin`, including only selected values.

The existing `--io-uring` and `--perf` flags are removed immediately. Invocations that still use them fail as unknown options.

## Permission semantics

### `io-uring`

Preserve the current opt-in io_uring policy under the new option.

Without `io-uring`, guest-init writes `2` to `kernel.io_uring_disabled`, preventing creation of new io_uring instances for all guest processes.

With `io-uring`, guest-init writes the dynamic guest `dev` GID to `kernel.io_uring_group` and keeps `kernel.io_uring_disabled` in restricted mode `1`. Processes in the `dev` group may create io_uring instances without `CAP_SYS_ADMIN`; other processes remain denied unless they meet the guest kernel's capability exception.

This permission does not grant a Linux capability. An independent explicit `sys-admin`
grant gives commands launched through `loftd-granted` `CAP_SYS_ADMIN`, which satisfies
the guest kernel's privileged io_uring exception without making `io-uring` imply that
capability.

### `perf`

Preserve the current performance-observability policy under the new option.

With `perf`, guest-init writes:

- `-1` to `kernel.perf_event_paranoid`
- `0` to `kernel.kptr_restrict`

Without `perf`, guest-init leaves the hardened guest defaults unchanged.

This permission does not grant `CAP_PERFMON`. Hardware performance events remain dependent on libkrun and virtual PMU support and are not guaranteed.

### `net-admin`

Grant `CAP_NET_ADMIN` to guest processes launched as `dev`.

This permits guest-scoped network administration such as interface, route, policy-routing, firewall, nftables, and TPROXY configuration where supported by the guest kernel and installed tools.

The capability is not granted to any host process or host namespace. It does not modify host networking.

### `bpf`

Grant `CAP_BPF` to guest processes launched as `dev`.

This permits privileged BPF operations governed by `CAP_BPF`. It does not imply or grant:

- `CAP_NET_ADMIN`
- `CAP_PERFMON`
- `CAP_SYS_ADMIN`

A BPF operation that requires another capability still requires the corresponding authority. Selecting both `bpf` and `net-admin` allows operations whose checks require both capabilities. Selecting `perf` changes the existing guest sysctls but does not add `CAP_PERFMON`.

Guest and nested-container seccomp policies remain independently applicable to the `bpf()` syscall.

### `sys-admin`

Authorize `CAP_SYS_ADMIN` independently for commands explicitly launched through
`loftd-granted`. This capability is exceptionally broad and remains absent from ordinary
`dev` commands. Selecting `bpf` or `io-uring` does not imply `sys-admin`; selecting
`sys-admin` independently satisfies kernel checks that accept `CAP_SYS_ADMIN`, including
the privileged exception for restricted io_uring creation.

## Selected capability-delivery approach

Use process capabilities during every transition from guest root to the `dev` workload identity.

For a `dev` workload, guest-init will:

1. Determine the requested capability set from the validated permission contract.
2. Ensure only the requested workload capabilities are retained for the child execution path.
3. Enable keep-caps across the UID transition.
4. Apply the existing supplementary-group, primary-GID, and UID transition to the dynamic `dev` identity.
5. Set the requested capabilities in the permitted, inheritable, and effective sets.
6. Raise the requested capabilities into the ambient set before executing the workload.
7. Ensure unrequested workload capabilities are absent.

Ambient capabilities are selected because they survive ordinary execution of non-privileged binaries and therefore work for arbitrary commands, shells, interpreters, and descendant processes without mutating executable files.

Capability setup is part of the same shared credential-transition API as the existing `dev` UID/GID drop. A requested capability operation that fails aborts the workload launch with contextual error output.

### Rejected alternatives

#### File capabilities

Setting file capabilities on selected executables is rejected because loftd launches arbitrary commands and interpreters. It would require predicting which binaries need privilege, mutate the task filesystem, and behave poorly for dynamically installed programs.

#### Running the workload as root

Running privileged workloads as guest root is rejected because it grants substantially more authority than requested and conflicts with loftd's normal `dev` workload model.

#### Initial-command-only capabilities

Granting capabilities only to the initial command is rejected because later `loftd exec` commands would not honor the task's launch permission contract.

## Workload scope

The selected Linux capabilities apply to all processes that loftd launches as guest user `dev` for the task:

- Initial direct workload execution.
- Initial managed PTY workload execution.
- Commands launched through the hidden `as-dev` transition from the task's privileged root shell.
- Every later non-PTY foreground command launched through `loftd exec`.
- Ordinary descendants of those processes, subject to normal Linux capability and executable-transition rules.

The permission set is task-scoped and immutable after launch. `loftd exec` does not accept a separate permission override.

A workload explicitly launched with loftd's existing root mode remains guest root and does not use the `dev` capability transition. The `io-uring` and `perf` selections still control their guest-wide kernel policy changes. This design does not attempt to reduce guest root to only the selected capabilities.

## Host-side architecture

Represent permissions as a typed set rather than four unrelated booleans.

The set crosses the existing host layers:

1. Clap parses `--permissions` into the typed set.
2. CLI conversion stores the set in `RuntimeOptions`.
3. Launch planning and session construction preserve the set.
4. `LaunchSpec` carries the set into guest configuration construction.
5. Serialized launch configuration preserves the guest configuration across helper and supervisor boundaries.
6. Guest configuration emits one canonical internal setting:

```text
LOFTD_PERMISSIONS=io-uring,net-admin,bpf,perf
```

Only selected values appear. When the set is empty, the marker may be omitted and guest-init treats absence as the empty secure default.

The previous `LOFTD_IO_URING` and `LOFTD_PERF` guest markers are removed together with the old CLI contract. No compatibility decoding is required.

## Guest-init architecture

Guest-init parses `LOFTD_PERMISSIONS` into its own typed permission set and validates every token again. It does not trust host-side validation as the sole boundary check.

Startup ordering remains fail-closed:

1. Parse and validate the guest environment contract.
2. Resolve the dynamic `dev` identity.
3. Apply existing guest hardening and selected `io-uring` and `perf` policy changes while guest-init is root.
4. Start existing background preparation and managed services.
5. Carry the immutable capability subset into workload launch state, including the managed exec listener.
6. Apply requested capabilities at each root-to-`dev` workload transition.
7. Execute the workload.

The direct command, managed PTY path, `as-dev` path, and exec listener must use one shared permission-aware credential transition rather than independently implementing capability manipulation.

The capability implementation should use the existing `libc` dependency and Linux capability syscalls/prctl operations unless a focused capability crate materially reduces error-prone low-level code. No dependency is added solely for convenience without reviewing its transitive and cargo-deny impact.

## Security boundaries

- No permission is enabled by default.
- `LOFTD_PERMISSIONS` is an internal host-to-guest launch contract, not a workload-controlled authorization API.
- `CAP_NET_ADMIN` and `CAP_BPF` are granted only inside the guest kernel.
- The host VM worker's capability set is unchanged.
- Host seccomp filters the host VM worker, not syscalls executed by guest processes in the guest kernel; selecting `bpf` therefore does not require adding `bpf()` to the host seccomp policy.
- Guest or nested-container seccomp can still deny `bpf()` independently.
- Nested Podman containers do not automatically inherit these capabilities. Container capability and seccomp configuration remain separate.
- `CAP_NET_ADMIN` is intentionally powerful within the guest and can change guest interfaces, routes, firewall state, and transparent-proxy policy.
- `CAP_BPF` increases the guest-kernel attack surface available to the workload and must remain explicit.
- Selecting `bpf` alone does not silently broaden authority by adding `net-admin`, `perf`, or `sys-admin` behavior.

## Error handling

Host CLI errors identify the invalid permission token and list the supported values.

Guest-init fails startup when:

- `LOFTD_PERMISSIONS` contains an unknown or empty token.
- A requested io_uring or perf sysctl cannot be opened or written.
- A requested capability cannot be retained across the UID transition.
- Capability set manipulation fails.
- An ambient capability cannot be raised.
- The guest kernel does not support a required capability or ambient-capability operation.

Capability errors identify the requested capability and failed operation. Guest-init never silently drops a selected capability and continues.

An unselected capability failing to exist or operate is irrelevant and does not affect launch.

## Testing

### Host CLI and domain tests

- Omitted `--permissions` produces an empty set.
- Each supported value parses independently.
- All values parse together.
- Input order does not affect equality or canonical serialization.
- Duplicate values are deduplicated.
- Unknown values fail with supported-value guidance.
- Empty values and empty entries fail.
- Removed `--io-uring` and `--perf` flags fail parsing.
- Runtime option conversion preserves the typed set.

### Launch-contract tests

- Launch planning preserves every selected permission.
- Guest configuration emits one canonical `LOFTD_PERMISSIONS` value.
- An empty set preserves the secure default.
- Launch-config serialization and deserialization preserve the canonical guest setting.
- Old `LOFTD_IO_URING` and `LOFTD_PERF` markers are no longer emitted.

### Guest parsing and policy tests

- Missing `LOFTD_PERMISSIONS` parses as an empty set.
- Every supported token and combination parses.
- Unknown and empty tokens fail.
- Canonical permission membership drives the existing io_uring and perf hardening components.
- Existing io_uring and perf behavior tests are migrated without changing their underlying policy expectations.

### Capability tests

Separate pure planning from privileged syscall execution so unit tests can verify:

- `net-admin` maps only to `CAP_NET_ADMIN`.
- `bpf` maps only to `CAP_BPF`.
- Selecting both produces exactly those capability bits.
- Selecting neither produces an empty capability plan.
- The credential/capability operation order keeps capabilities across UID change, applies the `dev` identity, sets requested sets, and raises ambient capabilities.
- Unrequested capabilities are absent.
- Errors from each capability operation include useful context.

Workload-path tests verify that the same permission set reaches:

- Direct execution.
- Managed PTY execution.
- Hidden `as-dev` execution.
- Managed `loftd exec` execution.

### Repository validation

Run:

```bash
nix develop --command cargo fmt --check
nix develop --command cargo clippy --all-targets --all-features -- -D warnings
nix develop --command cargo deny check
nix develop --command cargo test
```

### Live guest validation

When the environment permits a loftd live smoke test:

- Confirm an ordinary `dev` workload has neither `CAP_NET_ADMIN` nor `CAP_BPF`.
- Confirm `--permissions=net-admin` gives `dev` `CAP_NET_ADMIN` and not `CAP_BPF`.
- Confirm `--permissions=bpf` gives `dev` `CAP_BPF` and not `CAP_NET_ADMIN`.
- Confirm selecting both gives exactly both capabilities.
- Confirm a later `loftd exec` command observes the same selected capability set.
- Run a minimal BPF program-load probe appropriate for `CAP_BPF` to verify guest-kernel support.
- Where guest tooling is available, run a namespace-local nftables/TPROXY probe for the `net-admin` use case.

The live BPF probe must distinguish failure caused by a missing additional capability, unsupported program type, guest seccomp, or guest-kernel configuration from failure to deliver `CAP_BPF` itself.

## Documentation

Update `README.md` to:

- Replace `--io-uring` and `--perf` examples with `--permissions=io-uring` and `--permissions=perf`.
- Document all four supported values.
- State that the old flags were removed.
- Explain that `net-admin` and `bpf` grant powerful capabilities inside the guest VM only.
- Explain that `bpf` does not imply `net-admin` or `perf`.
- Explain that nested containers retain independent capability and seccomp policy.
- Retain the existing caveats about virtual PMU support and io_uring group policy.

## Acceptance criteria

- `loftd --permissions=io-uring,net-admin,bpf,perf` parses and reaches guest-init as one validated permission set.
- `--io-uring` and `--perf` are no longer accepted.
- Default launches retain the existing restricted io_uring and perf behavior and grant no workload capabilities.
- `io-uring` and `perf` preserve their current behavior under the unified option.
- `net-admin` grants only guest `CAP_NET_ADMIN` to all `dev` workload paths.
- `bpf` grants only guest `CAP_BPF` to all `dev` workload paths.
- Requested capability setup fails closed.
- Later `loftd exec` processes receive the task's selected capability set.
- Host capabilities, host networking, host seccomp, and nested Podman defaults remain unchanged.
- README and automated tests describe and enforce the breaking CLI contract.
