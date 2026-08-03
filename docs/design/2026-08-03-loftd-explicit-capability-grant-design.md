# Explicit Loftd Capability Grant Helper

## Summary

Loftd currently grants capability-bearing guest permissions to the normal `dev` workload process tree. This makes every descendant inherit capabilities, including Bubblewrap, which deliberately refuses non-root, non-setuid startup with a nonempty permitted capability set.

Replace task-wide process capability propagation with an explicit command boundary:

```sh
loftd-granted COMMAND [ARG...]
```

Normal workloads remain capability-free. When a task is launched with capability-bearing `--new-perms` values, guest-init installs a dedicated `loftd-granted` file-capability helper containing exactly those authorized capabilities. Invoking the helper grants all authorized capabilities to the requested command.

## Goals

- Keep normal initial workloads, managed exec commands, attached sessions, and their descendants free of effective, permitted, inheritable, and ambient capabilities.
- Preserve the existing host `--new-perms` interface.
- Keep `io-uring` and `perf` as task-wide kernel-policy permissions.
- Make `net-admin`, `net-raw`, and `bpf` available only through explicit `loftd-granted` invocation.
- Make the helper's kernel-enforced file-capability metadata the sole runtime source of capability authority.
- Protect all binaries under `/run/loftd/wrappers` with a read-only bind mount after initialization.
- Allow ordinary capability-free workloads to invoke Bubblewrap normally.

## Non-goals

- Selecting a subset of authorized capabilities per helper invocation.
- Granting capabilities to Bubblewrap itself.
- Making Bubblewrap setuid or attaching file capabilities to it.
- Renaming or splitting `--new-perms`.
- Changing the task-wide behavior of `io-uring` or `perf`.
- Making `loftd-granted` a sandbox between commands run by the same `dev` user.
- Defending the wrapper mount against a fully privileged guest-root process capable of remounting it.

## Selected approach

Use a dedicated static file-capability helper.

Alternatives considered were a setuid-root helper with a root-owned policy file and a privileged guest service. The file-capability helper is narrower: it does not execute as UID 0, requires no runtime policy IPC, and receives authority atomically from kernel-validated executable metadata.

The existing `loftd-guest-init` binary must not become the privileged helper. It exposes multiple initialization and internal subcommands; assigning capabilities to that multi-command binary would unnecessarily enlarge the privileged interface.

## Permission semantics

The existing permissions retain their names and host CLI syntax:

- `io-uring`: task-wide guest kernel-policy relaxation, unchanged.
- `perf`: task-wide guest kernel-policy relaxation, unchanged.
- `net-admin`: authorizes `CAP_NET_ADMIN` for `loftd-granted`.
- `net-raw`: authorizes `CAP_NET_RAW` for `loftd-granted`.
- `bpf`: authorizes `CAP_BPF` for `loftd-granted`.

Each `loftd-granted COMMAND` invocation receives all capability-bearing permissions authorized for the task. The helper accepts no capability selector.

If no capability-bearing permission was authorized, `loftd-granted` refuses to execute the command with a clear error.

## Guest initialization

While still root, guest-init:

1. Parses the trusted launch permission configuration.
2. Applies `io-uring` and `perf` policy behavior as today.
3. Computes the capability grant from only `net-admin`, `net-raw`, and `bpf`.
4. Restricts the task capability bounding set so it retains rootless-ID-map requirements and the authorized grant capabilities, while excluding unauthorized capabilities.
5. Launches normal `dev` workloads with empty effective, permitted, inheritable, and ambient capability sets.
6. Installs a dedicated static `loftd-granted` executable at `/run/loftd/wrappers/bin/loftd-granted` as `root:root`, mode `0555`.
7. Applies exactly the authorized file capabilities with effective and permitted flags.
8. Verifies the installed file's type, ownership, mode, and `security.capability` value.
9. Installs or verifies `newuidmap` and `newgidmap` as root-owned mode-`4755` helpers.
10. Bind-mounts `/run/loftd/wrappers` onto itself and remounts it read-only.

Guest startup fails closed if installation, capability assignment, metadata verification, or read-only remounting fails.

The wrapper mount remains executable and permits privilege transitions. It must not use `noexec` or `nosuid`; `nosuid` would disable both the subordinate-ID setuid helpers and file-capability acquisition.

## Wrapper layout

The final layout is:

```text
/run/loftd/wrappers             root:root, read-only bind mount
/run/loftd/wrappers/bin         root:root, 0755
/run/loftd/wrappers/bin/newuidmap   root:root, 4755
/run/loftd/wrappers/bin/newgidmap   root:root, 4755
/run/loftd/wrappers/bin/loftd-granted root:root, 0555, exact authorized file capabilities
```

The existing wrapper binary directory remains on the `dev` workload `PATH`.

## Runtime authorization

The `security.capability` xattr on the root-owned, read-only `loftd-granted` executable is the sole runtime source of truth.

The helper does not trust or parse:

- `LOFTD_PERMISSIONS`
- capability names from its command line
- a runtime policy file
- caller environment variables
- the capability bounding set as a grant list

On `execve()` of `loftd-granted`, the kernel derives the helper's capability sets from the executable xattr, subject to the bounding set. The helper then:

1. Rejects an empty command.
2. Reads its actual permitted capability set.
3. Refuses execution if the set is empty.
4. Refuses execution if the set contains anything outside `CAP_NET_ADMIN`, `CAP_NET_RAW`, and `CAP_BPF`.
5. Copies the complete permitted allowlisted set into its inheritable set.
6. Raises every capability in that set into its ambient set.
7. Executes the requested command without changing UID or GID.

The target command therefore receives exactly the capabilities the kernel granted to the helper. No user-controlled value can expand that set.

`CAP_SETPCAP` is not part of the designed grant. It may only be added if implementation-time kernel verification proves that moving already-permitted capabilities into inheritable and ambient sets requires it; otherwise it remains excluded.

## Capability behavior

Normal workload processes have zero values for:

- `CapEff`
- `CapPrm`
- `CapInh`
- `CapAmb`

The task bounding set can retain authorized capabilities because file-capability acquisition cannot exceed it. A nonempty bounding set alone does not give ordinary commands active capabilities.

A command invoked through `loftd-granted` intentionally starts a capability-bearing subtree. Descendants inherit according to normal Linux capability and `execve()` rules. Bubblewrap invoked inside that subtree will still reject the inherited permitted capabilities unless they are deliberately dropped first. Bubblewrap invoked from the normal workload tree will start capability-free and should work normally.

## Security properties

- The workload user cannot edit the helper, its directory, or its capability xattr.
- The read-only bind mount protects the completed wrapper tree from workload writes and accidental mutation.
- Copying the helper does not provide a way to mint a new privileged helper; the unprivileged user cannot install a privileged `security.capability` xattr.
- The helper cannot grant capabilities outside its kernel-assigned permitted set.
- The fixed allowlist makes unexpected privileged metadata fail closed.
- The bounding set is a defense-in-depth ceiling, not the runtime grant source.
- A fully privileged guest-root process can remount the wrapper tree and is outside this protection boundary.

## Error handling

Guest initialization fails before launching the workload when:

- The dedicated helper cannot be installed.
- Required ownership or mode cannot be established.
- The target filesystem does not support the required `security.capability` xattr.
- Installed capabilities differ from the selected launch capabilities.
- Existing wrapper metadata is unexpected.
- The wrapper tree cannot be bind-mounted and remounted read-only.

`loftd-granted` fails without executing the target when:

- No command is supplied.
- Its permitted capability set is empty.
- Its permitted set includes a capability outside the fixed allowlist.
- It cannot establish matching inheritable and ambient sets.
- Target command execution fails.

The helper is static to avoid dynamic-loader and preload behavior in a file-capability execution context.

## Testing

Behavioral tests belong under `crates/loftd-guest-init/src/` and cover:

- Permission-to-capability mapping.
- Rejection of capabilities outside the helper allowlist.
- Empty-grant refusal.
- Normal workload credential plans producing empty effective, permitted, inheritable, and ambient sets.
- Initial command, managed exec, and attached-session paths remaining capability-free.
- `io-uring` and `perf` not appearing in process capability sets.
- Exact helper ownership, mode, and capability metadata.
- `newuidmap` and `newgidmap` retaining `root:root` mode `4755`.
- Wrapper tree becoming a read-only executable mount without `nosuid`.
- Failure when capability xattrs or the read-only remount cannot be established.

Runtime acceptance checks cover:

- A task launched with `--new-perms=net-admin` has a capability-free normal shell.
- Ordinary `bwrap` starts successfully in that shell.
- `loftd-granted fish` receives `CAP_NET_ADMIN` and no unauthorized capabilities.
- `loftd-granted` refuses execution in a task without capability-bearing grants.
- `/run/loftd/wrappers` rejects writes while all three wrappers remain executable.

Standard validation is:

```sh
nix develop --command cargo fmt --check
nix develop --command cargo clippy --all-targets --all-features -- -D warnings
nix develop --command cargo deny check
nix develop --command cargo test
```

Because this changes guest privilege and mount behavior, live guest validation is required in addition to unit and workspace checks.
