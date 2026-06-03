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

**loftd**:
The future canonical CLI/runtime owner for direct-libkrun microvm task environments extracted from agentbox. During extraction, agentbox retains its existing microvm code until loftd is complete.
_Avoid_: agentbox microvm as the long-term owner

**single-runtime loftd CLI**:
The loftd command shape where `loftd` launches a microvm task directly because loftd owns only the direct-libkrun microvm runtime family. Runtime selection subcommands such as `loftd microvm` are unnecessary.
_Avoid_: preserving agentbox runtime selection in loftd

**host Podman exclusion**:
The loftd boundary that prevents the host CLI/runtime from depending directly on Podman for launch behavior. This exclusion does not ban rootless Podman preparation inside the guest environment.
_Avoid_: banning guest Podman tooling

**dynamic loftd host build**:
The packaging boundary where the host `loftd` binary is dynamically linked because it loads direct-libkrun runtime libraries supplied by the package or development shell.
_Avoid_: static loftd host artifact

**loftd prebuilt**:
The Nix package for a pinned neutral dynamic Linux `loftd-<arch>-unknown-linux-gnu` release asset; Nix patches ordinary ELF runtime dependencies and this repo wraps runtime tools plus the `libkrun`/`libkrunfw` library path.
_Avoid_: flake-locked release asset; static/standalone host loftd; pinned wrapper script

**static loftd guest init build**:
The packaging boundary where `loftd-guest-init` is a static/musl guest bootstrap binary because it runs inside the guest and does not load direct-libkrun host runtime libraries.
_Avoid_: dynamically linked loftd-guest-init image artifact

**loftd-guest-init**:
The future canonical guest init binary for loftd microvm task environments, extracted from agentbox guest init while preserving the same guest bootstrap role.
_Avoid_: agentbox-guest-init as the long-term loftd init name

**loftd contract naming**:
The runtime contract naming convention where loftd and loftd-guest-init use `LOFTD_*` environment variables, status names, and log identity. Loftd-guest-init does not accept legacy `AGENTBOX_*` aliases.
_Avoid_: compatibility aliases in loftd-guest-init

**compatibility handoff**:
The transition boundary where existing agentbox microvm behavior remains available until loftd and loftd-guest-init are complete, after which agentbox may delegate to loftd or remove the microvm command.
_Avoid_: early removal of agentbox microvm

**structure-preserving extraction**:
The extraction style where loftd mirrors agentbox-host's crate and module layout, and loftd-guest-init mirrors agentbox-guest-init's crate and module layout, while changing ownership and binary identity. It preserves reviewability rather than redesigning the architecture during extraction.
_Avoid_: opportunistic rearchitecture during extraction

**loftd state root**:
The loftd-owned runtime state location for task state, persistent cache disks, and related runtime state. By default it uses the loftd app namespace and can be redirected through loftd config without sharing agentbox state.
_Avoid_: reusing agentbox state for loftd

**loftd state config**:
The user config file that can override the base location for loftd runtime state, mirroring agentbox's `[state].location` shape under a loftd config namespace. It changes where loftd keeps runtime state without changing Buildah's normal containers configuration.
_Avoid_: Buildah config isolation knob

**workspace cache scope**:
The ownership boundary for microvm persistent cache disks. Persistent cache disks are scoped to the current workspace by default, with loftd using a separate `.loftd/` state root rather than sharing agentbox's `.agentbox/` state.
_Avoid_: global cache by default

**persistent cache disk**:
A shared guest disk that survives across microvm tasks to preserve expensive development caches while each task still receives a clean task root filesystem. Persistent cache disks include `/nix` and the container store.
_Avoid_: persistent root filesystem

**guest container tooling**:
Rootless container tools available inside the guest development environment. They remain part of loftd-guest-init because the Podman restriction applies to the loftd host run path, not to developer tools inside the guest.
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

**loftd-compatible image**:
An OCI image that contains the loftd-guest-init guest contract required to boot a loftd microvm task environment. Loftd defaults to loftd image identities rather than agentbox image identities.
_Avoid_: agentbox image as loftd default

**container flake output**:
The canonical Nix flake image output for the current extracted runtime owner. During loftd extraction, `.#container` names the loftd-compatible image, while `.#agentbox-container` names the legacy agentbox-compatible image variant.
_Avoid_: using `.#container` for the agentbox image after loftd owns the canonical image identity


**separate image publication**:
The release policy where agentbox-compatible images and loftd-compatible images keep distinct published identities. Agentbox image names are not compatibility aliases for loftd images while loftd is incomplete.
_Avoid_: shared image publication; publishing loftd-compatible images as agentbox images



**cache hit run**:
A microvm task launch where the selected OCI image is already available in the durable image source. A cache hit run should not pull image data, but the btrfs snapshot task rootfs backend may still use Buildah to mount the image rootfs source and create the task rootfs snapshot.
_Avoid_: image pull on every launch

**lazy image ingestion**:
The default source behavior where the first microvm run for an image ensures the selected OCI image is available in the durable image source if it is missing. Separate cache-management commands are optional future ergonomics, not required for v1 launch.
_Avoid_: mandatory prepare step

**image ingestion**:
The host-side preparation step that ensures the selected OCI image is available as a durable image source for later microvm task use. For loftd's btrfs snapshot default, ingestion should avoid duplicating the image rootfs into a separate loftd-owned extracted cache.
_Avoid_: VM launch, duplicated extracted rootfs by default

**rootless image ingestion**:
The expectation that a microvm cache miss can prepare the image cache without sudo. Host image ingestion may use rootless Buildah/user-namespace mechanisms, but it remains part of the normal rootless user experience.
_Avoid_: sudo-only cache miss

**Buildah image-source transaction**:
The single rootless user-namespace operation that resolves an OCI image, mounts a Buildah working rootfs, validates compatibility, and hands a snapshot-capable source to task rootfs materialization. Keeping the sequence together avoids mismatched Buildah storage and mount namespaces.
_Avoid_: split namespace image-source handling

**loftd image source boundary**:
The host preparation boundary where loftd may use Buildah to resolve and expose OCI-image root filesystems while keeping Podman out of the host run path. Buildah may reuse the user's normal containers configuration, such as `~/.config/containers`; a cache-hit btrfs-snapshot loftd launch may still use Buildah to mount the image rootfs source and create the task rootfs snapshot, but should not pull image data unless refresh is requested or the image is missing.
_Avoid_: Podman-backed loftd launch, duplicated extracted image rootfs by default

**loftd image refresh**:
The explicit image-refresh path where loftd may pull the canonical loftd image through Buildah, including `--pull-latest`. It preserves user ergonomics without reintroducing Podman-backed image operations.
_Avoid_: Podman pull for loftd

**rootless user contract**:
The expectation that normal agentbox and loftd commands run without sudo from the user's perspective. Microvm storage setup may have optional preparation paths, but normal task launch should remain rootless or fail with a clear diagnostic.
_Avoid_: sudo-only runtime


**outbound-first networking**:
The initial microvm networking scope: guest tasks need outbound network access, while general host port publishing is deferred until the direct networking model is proven.
_Avoid_: port publishing in v1

**microvm networking**:
The network path for a microvm task. The default should use libkrun-provided networking rather than adding a host-side helper process.
_Avoid_: passt by default

**libkrun port publishing**:
The default libkrun runtime's inbound network exposure from a host address or port to a guest task port. It is separate from microvm networking and uses the host runtime's publish-spec language.
_Avoid_: port bind, microvm port publishing


**libkrun FFI boundary**:
The host-side Rust boundary that calls libkrun directly for microvm task launch. Microvm starts with a narrow hand-written FFI surface wrapped by safe host code, rather than generated broad bindings.
_Avoid_: broad generated bindings by default

**libkrun discovery**:
The host-side mechanism that lets direct microvm boot load `libkrun.so` and its firmware dependency. Packaged agentbox should provide this automatically, while an explicit environment override remains available for source-build and debug workflows.
_Avoid_: manual linker setup as normal path

**run path**:
The critical execution path that starts a task environment. For **microvm**, the run path uses direct libkrun VM APIs rather than Podman, crun, or runc.


**durable image source**:
A per-user source of OCI image root filesystems that can be reused across workspaces. For loftd's btrfs-snapshot default, Buildah's image storage is the preferred durable image source instead of a duplicated loftd-owned extracted rootfs cache.
_Avoid_: per-workspace image extraction by default

**image source identity**:
The stable identity used for an OCI-image-derived root filesystem source. Image sources are identified by resolved image digest rather than mutable image tag.
_Avoid_: tag identity

**cached image rootfs**:
A workspace-independent extracted filesystem tree or subvolume derived from a compatible OCI image. It is not the preferred loftd btrfs-snapshot default path when Buildah's durable image source can be snapshotted directly.
_Avoid_: default duplicated image rootfs cache


**task rootfs lifecycle**:
The cleanup policy for a microvm task root filesystem. Task root filesystems are deleted after normal task exit by default, with explicit preservation for debugging.
_Avoid_: persistent task rootfs by default

**task rootfs backend**:
The host-side mechanism used to materialize a clean task root filesystem from an OCI-image-derived source. A task rootfs backend must preserve the clean task root filesystem contract without falling back to a plain recursive file copy.
_Avoid_: generic container storage


**reflink fast path**:
An agentbox-era explicit materialization path that requires a copy-on-write clone operation such as `cp -a --reflink=always`. It is not part of loftd's initial task rootfs backend set.
_Avoid_: loftd reflink backend by default

**btrfs snapshot default**:
The default loftd **task rootfs backend**. It gives each task a writable btrfs snapshot derived from a Buildah-mounted, snapshot-capable OCI image root filesystem. If btrfs snapshot storage is unavailable, loftd should fail clearly unless the user explicitly chooses another backend.
_Avoid_: automatic storage probing, recursive rootfs copy fallback



**packaged helper dependency**:
A host helper that agentbox should provide through its package or development shell when possible. For microvm, `fuse-overlayfs` is a packaged helper dependency for the fuse-overlay explicit fallback.
_Avoid_: hidden manual install requirement

**fuse-overlay explicit fallback**:
A non-btrfs **task rootfs backend** that a user may explicitly choose when btrfs snapshot storage is unavailable or undesired. It uses a real overlay view to preserve rootless copy-on-write behavior even though it adds a host helper dependency.
_Avoid_: silent automatic fallback

**task rootfs backend selection**:
The loftd policy where the task rootfs backend is selected deliberately through loftd configuration or a CLI override. Loftd's initial backend set is **btrfs snapshot default** and **fuse-overlay explicit fallback**; it does not include `auto` or `reflink`.
_Avoid_: container storage driver selection

## Example dialogue

Dev: Should this task use a named VM instance?
Domain expert: No. A microvm is task-based: each task gets a clean root filesystem derived from the image cache.

Dev: Is btrfs required?
Domain expert: Loftd defaults to btrfs snapshot storage and fails clearly if it cannot use it. Fuse-overlay is available only when the user explicitly chooses it through configuration or a CLI override.

Dev: Is Buildah forbidden?
Domain expert: Not for image ingestion, image-source mounting, or namespace-sensitive btrfs snapshot/delete commands. Btrfs-snapshot cleanup may still require the host btrfs mount option `user_subvol_rm_allowed` for rootless subvolume deletion.

Dev: Should `latest` name the cache?
Domain expert: No. The image source identity is the resolved digest; the original tag is only metadata.

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

Dev: Should users set `LD_LIBRARY_PATH` manually for `agentbox microvm`?
Domain expert: No. Libkrun discovery is a packaging responsibility for normal use, with an explicit environment override kept for source-build and debug workflows.

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

Dev: Does libkrun port publishing also change microvm networking?
Domain expert: No. Libkrun port publishing belongs to the default Podman-backed libkrun runtime; microvm keeps its outbound-first networking scope until a separate direct-libkrun decision changes it.

Dev: Can microvm boot arbitrary OCI images?
Domain expert: Not in v1. It requires an agentbox-compatible image with the guest init contract already present.

Dev: Must users prepare image caches before running?
Domain expert: No. Lazy image ingestion ensures the durable image source on first run if needed.

Dev: Is Buildah required for every microvm run?
Domain expert: No. A cache hit should not pull image data. A portable fuse-overlay cache-hit run may avoid Buildah if it has a durable extracted lowerdir, while the btrfs-snapshot path may still use Buildah to mount the durable image source and snapshot a task rootfs. Btrfs-snapshot cleanup also expects the backing btrfs mount to allow rootless subvolume removal with `user_subvol_rm_allowed`.

Dev: Can a microvm cache miss require sudo?
Domain expert: No. Rootless image ingestion means cache-miss preparation is part of the normal rootless user experience.

Dev: Should only `buildah mount` run inside `buildah unshare`?
Domain expert: No. Use one Buildah image-source transaction so image resolution, mounting, compatibility validation, task-rootfs snapshot creation, and cleanup share the same rootless namespace context. The btrfs task snapshot/delete path should also run through `buildah unshare`, and permission-denied delete failures should tell users to enable `user_subvol_rm_allowed` on the relevant btrfs mount rather than silently falling back to recursive cleanup.

Dev: Is the image cache per workspace?
Domain expert: No. The durable image source is per user and digest-addressed; mutable persistent cache disks stay per workspace.

Dev: Should a `btrfs` storage option exist?
Domain expert: No. Use the precise `btrfs-snapshot` name for real snapshot-backed task roots, and do not label generic rootfs copies as btrfs.

Dev: Should the explicit storage fallback be a plain copied rootfs?
Domain expert: No. The explicit storage fallback is a real fuse-overlay view: it accepts a host helper to keep rootless copy-on-write behavior.

Dev: Should loftd include a reflink task rootfs backend?
Domain expert: No. Loftd starts with btrfs-snapshot as the default and fuse-overlay as the explicit fallback; reflink remains outside the initial loftd backend set.

Dev: Should users manually install fuse-overlayfs?
Domain expert: Prefer no. Treat it as a packaged helper dependency when possible, with a clear error outside packaged environments.

Dev: Should v1 implement the whole microvm design in one pass?
Domain expert: No. Use milestone delivery: prove CLI, storage, direct boot, cache disks, then usability hardening in vertical slices.

Dev: Should loftd duplicate Buildah's btrfs image rootfs into a separate loftd-owned cached image rootfs by default?
Domain expert: No. For the btrfs-snapshot default, prefer Buildah as the durable image source and create the task rootfs snapshot directly from a Buildah-mounted rootfs when the btrfs preconditions are met.
