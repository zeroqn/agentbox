# Agentbox Context

Agentbox names runtime and storage concepts for launching clean task environments from OCI images.

## Language


**milestone delivery**:
The implementation approach for microvm that proves the runtime through small vertical slices rather than attempting every developer-environment contract at once.
_Avoid_: all-at-once implementation

**experimental runtime mode**:
A user-invoked runtime mode that is available for validation without becoming the default agentbox behavior. Microvm starts as an experimental runtime mode until its developer-environment contracts are proven.
_Avoid_: immediate default runtime

**microvm**:
The canonical user-facing runtime mode for launching an OCI-image-derived agentbox environment through direct libkrun VM APIs, distinct from Podman-backed container/runtime modes.
_Avoid_: akvm, avm, krunvm

**workspace cache scope**:
The ownership boundary for microvm persistent cache disks. Persistent cache disks are scoped to the current workspace by default, matching existing agentbox state isolation.
_Avoid_: global cache by default

**persistent cache disk**:
A shared guest disk that survives across microvm tasks to preserve expensive development caches while each task still receives a clean task root filesystem. Persistent cache disks include `/nix` and the container store.
_Avoid_: persistent root filesystem

**guest container tooling**:
Rootless container tools available inside the guest development environment. They are allowed because the microvm restriction applies to the host run path, not to developer tools inside the guest.
_Avoid_: host runtime dependency

**container store**:
A persistent cache disk for development container data inside the guest environment. It is preserved because microvm is a developer environment, not a disposable production sandbox.

**workspace mount**:
The current host working directory shared into a task environment at `/workspace`. It is the intentional project input/output boundary, not part of the clean task root filesystem.
_Avoid_: isolated project copy


**guest-visible runtime name**:
The runtime name exposed in guest init commands, environment variables, logs, and status. Microvm uses `microvm` as the guest-visible runtime name instead of reusing the existing Podman-backed `libkrun` name.
_Avoid_: libkrun label for microvm behavior


**guest-init override**:
A debugging path that lets a host-built guest init binary replace the image's guest init for a microvm task. It exists to shorten guest bootstrap development loops without rebuilding the OCI image.
_Avoid_: rebuild-only guest init testing

**guest init**:
The in-guest agentbox bootstrap program responsible for preparing the task environment before the task shell starts. Microvm reuses guest init rather than booting directly into a shell.
_Avoid_: direct shell boot


**terminal contract**:
The interactive task shell expectation that terminal size and resize behavior are good enough for normal developer use. Microvm v1 should include terminal resize support when libkrun exposes it, or document the limitation explicitly.
_Avoid_: broken interactive shell

**task shell**:
The default command experience for a microvm task. A task shell is an interactive shell inside the clean task environment, not the OCI image entrypoint contract.
_Avoid_: image entrypoint by default


**agentbox-compatible image**:
An OCI image that contains the guest init contract required to boot an agentbox task environment. Microvm v1 requires an agentbox-compatible image rather than adapting arbitrary OCI images at ingestion time.
_Avoid_: arbitrary image support in v1



**cache hit run**:
A microvm task launch where the digest-addressed image cache already exists. A cache hit run should not require Buildah because no image ingestion is needed.
_Avoid_: Buildah required for every run

**lazy image ingestion**:
The default cache behavior where the first microvm run for an image prepares the digest-addressed image cache if it is missing. Separate cache-management commands are optional future ergonomics, not required for v1 launch.
_Avoid_: mandatory prepare step

**image ingestion**:
The host-side preparation step that resolves an OCI image into a cached root filesystem for later microvm task use. Buildah is acceptable here because it is not the microvm run path.
_Avoid_: VM launch

**rootless user contract**:
The expectation that normal agentbox commands run without sudo from the user's perspective. Microvm storage setup may have optional preparation paths, but normal task launch should remain rootless or use a portable fallback.
_Avoid_: sudo-only runtime


**outbound-first networking**:
The initial microvm networking scope: guest tasks need outbound network access, while general host port publishing is deferred until the direct networking model is proven.
_Avoid_: port publishing in v1

**microvm networking**:
The network path for a microvm task. The default should use libkrun-provided networking rather than adding a host-side helper process.
_Avoid_: passt by default


**libkrun FFI boundary**:
The host-side Rust boundary that calls libkrun directly for microvm task launch. Microvm starts with a narrow hand-written FFI surface wrapped by safe host code, rather than generated broad bindings.
_Avoid_: broad generated bindings by default

**run path**:
The critical execution path that starts a task environment. For **microvm**, the run path uses direct libkrun VM APIs rather than Podman, crun, or runc.


**global image cache**:
A per-user cache of digest-addressed OCI image root filesystems that can be reused across workspaces. It is separate from workspace-scoped persistent cache disks because image cache entries are content-addressed and immutable-ish.
_Avoid_: per-workspace image extraction by default

**image cache identity**:
The stable identity used for a cached OCI-image root filesystem. Microvm image caches are identified by resolved image digest rather than mutable image tag.
_Avoid_: tag identity


**task rootfs lifecycle**:
The cleanup policy for a microvm task root filesystem. Task root filesystems are deleted after normal task exit by default, with explicit preservation for debugging.
_Avoid_: persistent task rootfs by default

**storage backend**:
The host-side mechanism used to materialize a clean task root filesystem from an OCI-image-derived cache.


**btrfs image subvolume**:
A global image cache entry stored as a btrfs subvolume when the btrfs fast path is available. It is the snapshot source for clean per-task root filesystems.

**btrfs task snapshot**:
A writable btrfs snapshot created from a btrfs image subvolume for one microvm task. It is deleted according to the task rootfs lifecycle.

**btrfs fast path**:
An optional **storage backend** that uses btrfs copy-on-write snapshots for per-task root filesystems when available.



**packaged helper dependency**:
A host helper that agentbox should provide through its package or development shell when possible. For microvm, `fuse-overlayfs` is a packaged helper dependency for the fuse-overlay fallback.
_Avoid_: hidden manual install requirement

**fuse-overlay fallback**:
The portable fallback storage backend for microvm task root filesystems when the btrfs fast path is unavailable. It uses `fuse-overlayfs` to preserve rootless copy-on-write behavior even though it adds a host helper dependency.
_Avoid_: plain copy fallback

**portable fallback**:
A non-btrfs **storage backend** used when the **btrfs fast path** is unavailable or not requested. For microvm v1, this means the **fuse-overlay fallback**.
_Avoid_: mandatory fallback

## Example dialogue

Dev: Should this task use a named VM instance?
Domain expert: No. A microvm is task-based: each task gets a clean root filesystem derived from the image cache.

Dev: Is btrfs required?
Domain expert: No. Use the btrfs fast path when available, otherwise use a portable fallback.

Dev: Is Buildah forbidden?
Domain expert: Not for image ingestion. It is acceptable for unpacking or caching OCI images, but not for the microvm run path.

Dev: Should `latest` name the cache?
Domain expert: No. The image cache identity is the resolved digest; the original tag is only metadata.

Dev: Does microvm run the image entrypoint by default?
Domain expert: No. A microvm task opens a task shell by default; image entrypoint semantics can be a later explicit mode.

Dev: Can microvm skip guest init and boot bash directly?
Domain expert: No. Guest init remains the in-guest bootstrap before the task shell starts.

Dev: Is the project directory copied into each task?
Domain expert: No. The workspace mount shares the current host working directory at `/workspace`; the clean boundary is the task root filesystem.

Dev: Does a clean task mean all guest state is disposable?
Domain expert: No. The task root filesystem is clean, but persistent cache disks preserve `/nix` and the container store for developer productivity.

Dev: Are persistent cache disks shared across all projects?
Domain expert: No. The workspace cache scope keeps persistent cache disks per workspace by default.

Dev: Does avoiding Podman on the host mean no containers inside the VM?
Domain expert: No. Guest container tooling remains available inside the dev environment; only the host run path avoids Podman/crun/runc.

Dev: Should microvm start passt on the host?
Domain expert: Not by default. Microvm networking should use libkrun-provided networking first.

Dev: Can microvm require `sudo` to launch tasks?
Domain expert: No. The rootless user contract means normal task launch stays rootless from the user's perspective.

Dev: Should microvm replace the default runtime immediately?
Domain expert: No. Microvm starts as an experimental runtime mode until its developer-environment contracts are proven.

Dev: Should direct libkrun use generated bindings?
Domain expert: No, not initially. The libkrun FFI boundary should be narrow and hand-written for v1.

Dev: Can terminal resizing wait until later?
Domain expert: Only if libkrun cannot support it cleanly. The terminal contract is part of a usable v1 task shell.

Dev: Does a task root filesystem survive after exit?
Domain expert: No, not normally. The task rootfs lifecycle deletes it after normal exit, with explicit preservation for debugging.

Dev: Should guest logs still call this runtime `libkrun`?
Domain expert: No. The guest-visible runtime name is `microvm`, even when implementation helpers are shared with existing libkrun code.

Dev: Should guest init overrides be removed from microvm?
Domain expert: No. A guest-init override is important for debugging direct runtime bring-up without rebuilding the image.

Dev: Does v1 need `--publish` port forwarding?
Domain expert: No. Use outbound-first networking for v1; general port publishing can come later.

Dev: Can microvm boot arbitrary OCI images?
Domain expert: Not in v1. It requires an agentbox-compatible image with the guest init contract already present.

Dev: Must users prepare image caches before running?
Domain expert: No. Lazy image ingestion prepares the image cache on first run if needed.

Dev: Is Buildah required for every microvm run?
Domain expert: No. A cache hit run does not require Buildah; Buildah is required only when image ingestion is needed.

Dev: Is the image cache per workspace?
Domain expert: No. The global image cache is per user and digest-addressed; mutable persistent cache disks stay per workspace.

Dev: Where does the btrfs fast path apply?
Domain expert: To both the global image cache and per-task rootfs: image cache entries are btrfs image subvolumes, and task roots are btrfs task snapshots.

Dev: Should the portable fallback be a plain copied rootfs?
Domain expert: No. The portable fallback is a fuse-overlay fallback: it accepts a host helper to keep rootless copy-on-write behavior.

Dev: Should users manually install fuse-overlayfs?
Domain expert: Prefer no. Treat it as a packaged helper dependency when possible, with a clear error outside packaged environments.

Dev: Should v1 implement the whole microvm design in one pass?
Domain expert: No. Use milestone delivery: prove CLI, storage, direct boot, cache disks, then usability hardening in vertical slices.
