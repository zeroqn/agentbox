# Opt-in guest io_uring design

## Summary

loftd will disable io_uring inside the guest VM by default and expose a top-level `--io-uring` boolean to allow the dynamic guest `dev` group to use it for an individual VM launch.

The policy will be enforced through `/proc/sys/kernel/io_uring_disabled` and `/proc/sys/kernel/io_uring_group`:

- Default launch: write `2` to `io_uring_disabled`, preventing all guest processes, including root, from creating io_uring instances.
- `loftd --io-uring`: write the dynamic `dev` GID to `io_uring_group`, then keep `io_uring_disabled` at restricted mode `1`. Processes in that group can create io_uring instances without `CAP_SYS_ADMIN`; other processes remain denied unless they satisfy the kernel capability exception.

The setting will be applied fail-closed by loftd guest-init before background services or the user workload start.

## Goals

- Reduce the default guest-kernel attack surface by disabling io_uring.
- Permit explicit per-launch opt-in for workloads that require io_uring.
- Apply one consistent default policy to all processes and a group-scoped opt-in to the guest `dev` identity.
- Keep host seccomp, host Landlock, and nested-container seccomp boundaries independent.
- Make failure to enforce the requested policy abort guest initialization.

## Non-goals

- Changing loftd's host VM-worker seccomp policy.
- Changing loftd's host Landlock policy.
- Automatically changing nested Podman seccomp policy.
- Providing arbitrary per-process or per-container io_uring controls.
- Maintaining separate guest kernels with and without io_uring support.

## Alternatives considered

### Guest-kernel sysctls

Use `kernel.io_uring_disabled` and `kernel.io_uring_group` during guest initialization.

Advantages:

- Controls the guest-kernel boundary where guest io_uring syscalls execute.
- Covers all guest processes with the default-disabled policy and permits only the dynamic `dev` group when opted in.
- Requires no additional process wrapper or guest seccomp layer.
- Supports per-VM opt-in without maintaining multiple kernels.

Disadvantages:

- The opt-in is group-scoped rather than guest-wide; processes outside the `dev` group remain denied unless they satisfy the kernel's `CAP_SYS_ADMIN` exception.
- Requires a guest kernel that exposes both sysctls.

This is the selected approach.

### Guest workload seccomp

Install a guest-side seccomp filter around the final workload.

Advantages:

- Could provide per-process control.

Disadvantages:

- Does not naturally cover guest services or alternate execution paths.
- Adds another confinement system and significant launch-path complexity.
- Is easier to bypass accidentally as guest execution paths evolve.

This approach is rejected for the first version.

### Kernel built without io_uring

Compile io_uring out of the guest kernel.

Advantages:

- Removes the io_uring implementation from the guest attack surface entirely.

Disadvantages:

- Cannot provide a per-launch opt-in without maintaining and selecting between multiple kernels.

This approach is rejected because opt-in support is required.

### Restricted group mode

Set `io_uring_group` to the dynamic guest `dev` GID while preserving the guest kernel's boot-time `io_uring_disabled=1` restricted mode.

Advantages:

- Permits the normal guest workload identity without requiring `CAP_SYS_ADMIN`.
- Retains the guest kernel's restricted boot-time mode instead of attempting the unsupported transition to `0`.
- Keeps processes outside the `dev` group denied unless the kernel capability exception applies.

Disadvantages:

- Requires the dynamic `dev` GID to be resolved before the policy is applied.
- Nested-container access still depends on GID mapping and the container's independent seccomp policy.

This is the selected opt-in mode; default launches still fully disable new rings with `io_uring_disabled=2`.

## CLI behavior

Add a top-level boolean option:

```text
--io-uring
```

Semantics:

- Omitted: io_uring is disabled for all processes in the guest VM.
- Present: processes in the dynamic guest `dev` group may create io_uring instances without `CAP_SYS_ADMIN`; other processes remain restricted by the kernel policy.

The help text will state that the option permits the guest `dev` group and leaves host seccomp, host Landlock, and nested Podman seccomp unchanged.

The option applies to normal task launches. Maintenance commands retain the secure disabled default unless a concrete maintenance workload requires otherwise in a future design.

## Architecture and data flow

### Host CLI and launch planning

The selected boolean will flow through the existing host launch structures:

- CLI parse result and `LaunchOptions`
- normal launch planning
- `LaunchSpec`
- `LaunchConfig`

The launch configuration will serialize the value explicitly so helper and supervisor boundaries preserve the selected mode.

### Guest bootstrap environment

When enabled, the host will add:

```text
LOFTD_IO_URING=1
```

Absent or any value other than `1` means disabled, following the existing guest boolean environment convention.

The explicit serialized launch field remains the host contract source of truth; guest environment construction derives the guest-init signal from it.

### Guest-init hardening

Add an io_uring hardening component alongside the existing dmesg hardening component.

It will write to:

```text
/proc/sys/kernel/io_uring_disabled
/proc/sys/kernel/io_uring_group
```

Requested values:

- Disabled: write `2\n` to `io_uring_disabled`.
- Enabled: write the dynamic `dev` GID to `io_uring_group` and leave the boot-time `io_uring_disabled=1` value unchanged. Rewriting that monotonic sysctl, even to the same value, returns `EINVAL` on the bundled kernel.

Guest-init will apply the setting while still running as root, before:

- Nix background preparation
- Podman background preparation
- Wayland proxy startup
- managed-session startup
- final workload execution

This ordering prevents normal guest services or workloads from creating rings before the selected policy is installed.

## Error handling

Guest initialization will fail if either required sysctl cannot be opened or written.

The error will identify:

- `/proc/sys/kernel/io_uring_disabled` or `/proc/sys/kernel/io_uring_group`
- the requested value
- the underlying I/O failure

There will be no fallback to:

- leaving the kernel default or group unchanged;
- globally enabling io_uring with `io_uring_disabled=0`;
- guest workload seccomp.

Failing closed avoids starting a VM with a weaker or different io_uring policy than the user selected.

## Host seccomp interaction

No changes are required to loftd's packaged default host seccomp policy.

The existing loftd policy filters syscalls made by the host VM-worker process. Syscalls issued by guest processes execute in the guest kernel and do not pass through the host seccomp filter.

Therefore `--io-uring` will not add these syscalls to the host policy:

- `io_uring_setup`
- `io_uring_enter`
- `io_uring_register`

Existing host options remain independent:

- `--seccomp=off`
- audit and trace modes
- custom enforce policies

## Host Landlock interaction

No changes are required to loftd's host Landlock policy or ordering.

Host Landlock confines the host VM worker and does not directly confine processes inside the guest kernel. Allowing the guest `dev` group to create io_uring instances does not create a host io_uring ring and does not inherit host credentials.

Guest access to host-backed resources such as `/workspace` still crosses the VM device/backend and remains subject to the host worker's existing Landlock boundary.

The opt-in nevertheless increases guest-kernel attack surface, so it remains explicit and disabled by default.

## Future guest Landlock caveat

If loftd later applies Landlock inside the guest, it must establish the guest Landlock domain before untrusted code can create io_uring rings or register personalities.

The sysctl only prevents creation of new rings. Existing rings remain usable after the sysctl changes. An io_uring personality registered before `landlock_restrict_self()` can retain credentials from before the Landlock restriction and must not cross into an untrusted process boundary.

The proposed current boot ordering prevents normal user-space guest rings from predating the io_uring policy, but any future guest Landlock design must independently preserve safe ring and credential ordering.

## Nested Podman interaction

The packaged nested-container seccomp profile remains unchanged.

The currently pinned container-libs default profile does not allow:

- `io_uring_setup`
- `io_uring_enter`
- `io_uring_register`

Consequently, `loftd --io-uring` permits a nested process only if its mapped group qualifies as the guest `dev` GID and its container seccomp policy permits the io_uring syscalls. Users who need io_uring inside a nested container must separately select or provide an appropriate container seccomp policy.

This separation avoids broadening every nested container's syscall surface as a side effect of enabling the guest-kernel group policy.

## Testing

### Host CLI tests

- Verify the default is disabled.
- Verify `--io-uring` parses as enabled.
- Verify help text describes dev-group scope and the independent host/container policy boundaries.

### Launch planning and contract tests

- Verify the enabled state flows through launch planning.
- Verify launch-config serialization and parsing preserve both enabled and disabled values.
- Verify enabled launches emit `LOFTD_IO_URING=1` to guest-init.
- Verify default launches do not emit `LOFTD_IO_URING`.
- Verify maintenance launch configurations retain the disabled default.

### Guest-init tests

- Verify disabled mode writes exactly `2\n` to `io_uring_disabled` and leaves `io_uring_group` unchanged.
- Verify enabled mode writes the dynamic `dev` GID to `io_uring_group` without writing `io_uring_disabled`.
- Verify existing sysctl contents are overwritten.
- Verify open and write failures for both sysctls propagate with useful context.
- Verify missing `LOFTD_IO_URING` resolves to disabled.
- Verify `LOFTD_IO_URING=1` resolves to enabled.

### Startup ordering

Verify io_uring hardening occurs before:

- Nix preparation;
- Podman preparation;
- Wayland proxy startup;
- managed-session startup;
- final workload exec.

### Repository validation

Run:

```bash
nix develop --command cargo fmt --check
nix develop --command cargo clippy --all-targets --all-features -- -D warnings
nix develop --command cargo deny check
nix develop --command cargo test
```

### Manual VM smoke

- Default launch: an `io_uring_setup` probe returns `EPERM` as the normal guest user.
- Default launch: the same probe returns `EPERM` as guest root.
- `loftd --io-uring`: the probe successfully creates an io_uring instance in the guest shell.
- With `--io-uring`, a nested Podman container remains denied by its default seccomp profile.

## Expected affected areas

- `crates/loftd/src/cli/`
- `crates/loftd/src/runtime/launch/`
- `crates/loftd-guest-init/src/guest_init/components/hardening/`
- `crates/loftd-guest-init/src/guest_init/runtime/loftd.rs`
- Relevant host and guest tests
- `README.md`

No host seccomp policy, host Landlock policy, or nested-container seccomp policy files should change.
