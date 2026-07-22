# Opt-in loftd guest performance profiling

## Summary

Add an explicit top-level `loftd --perf` option that relaxes the guest kernel's performance-event policy for that launch. The hardened default remains unchanged. Add `perf` and `strace` to the loftd guest image so optimization and diagnostic workflows are available without installing tools at runtime.

## Goals

- Preserve `kernel.perf_event_paranoid=3` for ordinary loftd launches.
- Allow an explicitly opted-in guest workload to use kernel software events and tracepoints required for end-to-end io_uring profiling.
- Keep performance profiling independent from permission to create io_uring instances.
- Include `perf` and `strace` in the loftd image only.
- Clearly document that hardware performance counters remain dependent on virtual PMU support.

## Non-goals

- Expose or implement a virtual PMU in libkrun.
- Guarantee hardware events such as CPU cycles, instructions, cache misses, or branch misses.
- Change host seccomp, host Landlock, nested Podman seccomp, or guest io_uring policy.
- Enable perf access automatically with `--profile`, `--io-uring`, or root mode.
- Add the profiling tools to the agentbox image.

## User-visible behavior

### Default launch

Without `--perf`, loftd retains the guest kernel default:

```text
kernel.perf_event_paranoid=3
```

No performance-monitoring relaxation occurs.

### Opt-in launch

With `--perf`, loftd guest-init writes:

```text
-1
```

to:

```text
/proc/sys/kernel/perf_event_paranoid
```

This permits unprivileged guest processes to access kernel performance events and raw tracepoints where supported by the guest kernel. The option intentionally weakens performance-event isolation for that VM launch.

For end-to-end io_uring profiling, the expected invocation is:

```bash
loftd --io-uring --perf
```

The options remain independent:

- `--io-uring` controls whether processes in the guest `dev` group can create io_uring instances.
- `--perf` controls guest performance-event observability.

### Hardware counter limitation

The guest kernel enables `CONFIG_PERF_EVENTS`, but the current x86 libkrun CPUID transformation disables the architectural PMU. Therefore `--perf` enables software events, kernel profiling, and tracepoints where available, but does not promise hardware events such as cycles or instructions. Virtual PMU exposure requires a separate design.

## Architecture and data flow

The new boolean follows the existing loftd runtime-option contract:

1. The top-level Clap CLI parses `--perf`, defaulting to `false`.
2. CLI conversion places the value in runtime options.
3. Launch planning and session construction preserve it in the launch model.
4. The serialized launch configuration preserves the value across helper and supervisor boundaries.
5. Guest configuration serialization emits `LOFTD_PERF=1` only when enabled.
6. loftd guest-init parses the marker as a boolean, defaulting to `false` when absent.
7. Root bootstrap applies the sysctl before Nix preparation, Podman preparation, Wayland startup, managed-session startup, privilege drop, or workload execution.

Older or omitted serialized launch fields decode to the secure `false` default. New launch configurations serialize the field explicitly where the existing launch codec requires explicit fields.

## Guest sysctl component

Add a focused hardening component for `perf_event_paranoid`, following the existing dmesg and io_uring component patterns.

The component owns:

```text
/proc/sys/kernel/perf_event_paranoid
```

Behavior:

- Disabled: perform no write and retain the guest kernel's value of `3`.
- Enabled: write exactly `-1\n`.
- Open or write failure: fail guest initialization with context containing the sysctl path, setting name, and attempted value.

The setting is applied only while guest-init is root. It is placed alongside the existing early hardening operations because the sysctl remains mutable after boot and must be configured before the user workload begins.

## CLI documentation

The `--perf` help text will explain:

- The option sets `kernel.perf_event_paranoid=-1` inside the guest for that launch.
- It enables guest kernel software events and tracepoints useful for application and io_uring analysis.
- It weakens guest performance-event isolation.
- It does not alter io_uring creation policy; combine it with `--io-uring` when needed.
- Hardware PMU events depend on libkrun and host virtualization support and are not guaranteed.

The README will include the io_uring profiling example and the hardware-counter limitation.

## Guest image packaging

Add the nixpkgs `perf` and `strace` packages to the loftd-specific image package selection next to the existing loftd-only Wayland proxy package.

Requirements:

- Both binaries are included in the loftd image closure.
- Both binaries are available on the guest PATH.
- Neither package is added to the agentbox image.
- Image Nix DB metadata continues to cover all referenced store paths.

The selected `perf` userspace package comes from the repository's pinned nixpkgs. Exact kernel/userspace version matching is not required for the initial scope because the acceptance criteria focus on standard software events and tracepoints supported by the running guest kernel. Any concrete incompatibility found during image validation must be resolved before implementation is considered complete.

## Security considerations

`perf_event_paranoid=-1` grants broad performance-event access inside the guest. This can expose kernel activity and information about other guest processes. The risk is constrained by:

- An explicit per-launch option.
- A hardened default of `3`.
- The short-lived, isolated guest VM boundary.
- No changes to host seccomp, Landlock, or nested container policy.

The feature is not coupled to unrelated convenience or profiling flags, preventing accidental relaxation.

## Testing

### Host crate

- CLI defaults `perf` to false.
- `--perf` is accepted as a top-level launch option.
- Launch models preserve enabled and disabled values.
- Guest environment contains `LOFTD_PERF=1` only when enabled.
- Serialized configurations missing the field decode as false where compatibility decoding applies.
- Existing management-subcommand option behavior remains unchanged.

### Guest crate

- Environment parsing defaults `LOFTD_PERF` to false.
- A present marker enables it.
- Enabled configuration writes exactly `-1\n`.
- Disabled configuration performs no write.
- Missing and unwritable sysctl paths return contextual errors.
- Runtime bootstrap invokes perf configuration before workload preparation and execution.

### Nix image checks

- The loftd image exposes `perf` and `strace` on PATH.
- The agentbox image does not gain either loftd-only package through this change.
- Image closure and Nix DB metadata checks continue to pass.

## Validation

Run targeted tests first, followed by the repository validation sequence:

```bash
nix develop --command cargo test -p loftd perf_
nix develop --command cargo test -p loftd-guest-init perf_
nix develop --command cargo fmt --check
nix develop --command cargo clippy --all-targets --all-features -- -D warnings
nix develop --command cargo deny check
nix develop --command cargo test
nix build .#checks.x86_64-linux.container-wrapper-contracts
nix build .#container
```

Live smoke validation should verify:

- Without `--perf`, `/proc/sys/kernel/perf_event_paranoid` remains `3`.
- With `--perf`, it is `-1`.
- `strace` starts successfully in the guest.
- `perf` can execute a supported software-event or tracepoint workflow.
- `loftd --io-uring --perf` permits an io_uring workload and its kernel-path profiling where the guest exposes the relevant tracepoints.

Hardware events are not an acceptance criterion because current libkrun x86 CPUID handling disables the virtual PMU.
