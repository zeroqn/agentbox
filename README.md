# agentbox

`agentbox` is a small Rust CLI that starts an interactive Podman container shell
for your current project.

It mounts the current directory at `/workspace`, persists Codex/Cargo state on
the host, and runs Nix inside the default libkrun guest runtime.

> **Runtime notice:** default libkrun mode is not a stock Podman runtime. It is
> intended to run with this flake's custom `crun`/Podman build and the pinned
> `libkrunfw` firmware package, which provide the `krun` handler, raw data disk
> annotations, and nested-KVM firmware support used by agentbox.

Current runtime split:

- **Libkrun mode (default):** Podman + crun/libkrun VM mode with two sparse
  raw btrfs data images attached through `krun.disk.*` annotations. The guest
  uses disk 0 for a persistent kernel overlay at `/nix` and disk 1 for
  rootless Podman storage as `dev` with the `btrfs` storage driver. The image
  also provides `docker` and `docker-compose` compatibility commands backed by
  Podman rather than a Docker daemon. Libkrun shell entry starts rootless
  Podman preparation in the background; direct `podman` waits only for that
  prep to finish, while `docker` and `docker-compose` additionally start or
  repair the Podman API socket on first use.
  The `/workspace` bind mount uses `--userns=keep-id` so ownership matches the
  host user after the guest drops privileges.
- **Container mode (`agentbox container`):** native Podman task container plus
  host `fuse-overlayfs` and a reusable `nix-daemon` sidecar.
  `agentbox container sidecar` starts or reuses only the sidecar stack for
  debugging.
- **Microvm mode (`agentbox microvm`, experimental):** task-based direct-libkrun
  runtime branch for one-shot microVM runs from an OCI image cache. It prepares
  a clean per-task rootfs, attaches per-workspace sparse btrfs disks for `/nix`
  and rootless container storage, supervises a same-binary helper that calls
  libkrun directly, runs `agentbox-guest-init microvm enter`, and mounts the
  current workspace at `/workspace` through virtiofs.

Seeded `/nix` copy fallback has been removed. Container mode always uses the
managed sidecar.

---

## Prerequisites

- Linux
- `podman`
- `nix` (for building via flake)
- `fuse-overlayfs` (required for `agentbox container` sidecar mode and for
  `agentbox microvm --storage fuse-overlay`; included by the
  `.#agentbox-prebuilt` package runtime environment)
- `buildah` for experimental `agentbox microvm` cache misses, for `agentbox
  microvm --storage btrfs-snapshot` task-rootfs snapshot/delete operations, and
  for loftd's default `btrfs-snapshot` Buildah image-source transaction;
  included by the Nix `.#agentbox` and `.#agentbox-prebuilt` package runtime
  environments. Agentbox cache-miss ingestion is
  rootless and runs as one `buildah unshare` transaction so Buildah storage,
  mount, copy, and cleanup share the same user namespace; the Rust ingestion
  child creates a btrfs subvolume cache rootfs when supported, then copies with
  `cp -a --reflink=auto` so CoW filesystems can avoid full data copies while
  non-reflink filesystems keep the portable fallback. Existing digest-keyed
  microvm cache hits do not require Buildah unless the selected storage backend
  is `btrfs-snapshot`.
- `libkrun.so` at runtime for experimental `agentbox microvm` and `loftd`
  direct boot. The normal host binaries do not link to libkrun at build time;
  the Nix `.#agentbox` package wraps the agentbox binary with this repo's
  libkrun/libkrunfw library path, while the Nix `.#loftd` source package keeps
  `bin/loftd` as a raw ELF and resolves libkrun from `$out/lib/loftd` before
  falling back to sonames. Source/debug builds can set
  `AGENTBOX_LIBKRUN_LIBRARY=/path/to/libkrun.so.1` for agentbox microvm or
  `LOFTD_LIBKRUN_LIBRARY=/path/to/libkrun.so.1` for loftd.
- `pasta`/`passt` for loftd direct-libkrun host-alias networking in both
  default TSI and `--passt` mode; included in the Nix `.#loftd` helper dir,
  `.#loftd-prebuilt`, and `nix develop` environments.
- Linux Landlock enabled in the host kernel for default `loftd` task launches.
  Ordinary launches now use host-side Landlock `relax` mode by default; use
  `--landlock=all` for stricter TCP bind handling,
  `--landlock=best-effort` on older/degraded kernels, or `--landlock=off` as an
  explicit debugging escape hatch.
- The packaged loftd default seccomp policy at
  `$out/share/loftd/seccomp/default.json` for ordinary `loftd` task launches
  that omit `--seccomp`; source-built and prebuilt loftd packages install this
  file.
- `strace` for explicit `loftd --seccomp=audit:<trace>` policy-discovery
  runs. It is included in the Nix `.#loftd` helper dir and `nix develop`
  environments. Audit mode uses ptrace on the loftd VM worker only; normal
  child tracing should work with `kernel.yama.ptrace_scope=1`, but hosts that
  disable ptrace entirely must allow ptrace for the audit run.
- `btrfs`, `mkfs.btrfs`, and `blkid` on the host for microvm btrfs-snapshot
  storage, first-time libkrun/microvm raw-image creation, and reuse validation
  (`btrfs-progs` + `util-linux`; included in `nix develop`, included in the
  Nix `.#loftd` helper dir, and `btrfs` is included in the Nix `.#agentbox` and
  `.#agentbox-prebuilt` runtime wrappers).
  Task-rootfs btrfs snapshot and delete commands run through `buildah unshare`.
  Rootless btrfs-snapshot cleanup also requires the backing btrfs mount to allow
  user-owned subvolume removal; add `user_subvol_rm_allowed` to that mount's
  options when using this fast path.
- `/dev/net/tun` on the host for libkrun mode, passed through to the guest so
  nested rootless Podman can set up TUN-backed networking.
- default libkrun mode requires Podman using the custom crun/libkrun stack that
  supports `krun_add_disk` annotations plus guest kernel overlay and btrfs
  support.

---

## Development

```bash
nix develop
cargo build
cargo test
```

`nix develop` opens `fish` + `starship` by default. Keep your current shell:

```bash
AGENTBOX_DISABLE_AUTO_FISH=1 nix develop
```

Inside the agentbox container, `nix` is invoked through a small compatibility
wrapper that clears the entrypoint's NSS wrapper preload before running the real
Nix binary. This prevents nested dev shells from mixing the container NSS preload
with a different glibc from the shell's realized dependencies. If you are using
an older image without that wrapper, use this temporary workaround:

```bash
env -u LD_PRELOAD -u NSS_WRAPPER_PASSWD -u NSS_WRAPPER_GROUP nix develop
```

The container defaults Nix-linked dynamic binaries to `mimalloc` through
`/etc/ld-nix.so.preload`, matching NixOS' allocator preload mechanism rather
than setting a global allocator `LD_PRELOAD`. Pass `--hardened` to `agentbox` or
`loftd` task runs to have guest-init rewrite `/etc/ld-nix.so.preload` to
GrapheneOS `hardened_malloc` instead. The image records both allocator paths in
`/etc/nix-allocator-libs`; the host passes only the allocator mode selector.
`rustc` and `rust-analyzer` are started through wrappers that mask
`/etc/ld-nix.so.preload` for those processes so they keep the default allocator.
Foreign/FHS glibc binaries usually read `/etc/ld.so.preload` instead of
`/etc/ld-nix.so.preload`, while static or musl binaries generally ignore both
files. For a specific foreign/FHS command, opt in to GrapheneOS
`hardened_malloc` with:

```bash
hardening-run some-foreign-binary --flag
```

`hardening-run` sets `LD_PRELOAD` only for the wrapped command. The in-image
`agentbox-guest-init` and `loftd-guest-init` binaries are the static musl
bootstrap path that materializes the selected preload file; dynamic
`--guest-init` overrides are not guaranteed to run under GrapheneOS
`hardened_malloc` until after they have started and rewritten
`/etc/ld-nix.so.preload`. The usual opt-out remains:

```bash
env -u LD_PRELOAD some-foreign-binary --flag
```

---

## Build

```bash
nix build .#loftd
nix build ./nix/dev#loftd-dev
nix build .#agentbox-prebuilt
nix build .#loftd-prebuilt
nix build .#agentbox-musl
nix build .#rmux-prebuilt
nix build .#rtk-prebuilt
nix build .#libkrunfw
nix build .#libkrun
nix build .#crun
nix build .#podman
nix build .#container-lib-policy-seccomp-json
nix build .#container
nix build .#agentbox-container
```

CI publishes release artifacts on every push to `main` and on every git tag
(`v*`):

- **Rolling** (branch push to `main`): `agentbox-<arch>-unknown-linux-musl`
  and `loftd-<arch>-unknown-linux-gnu` are uploaded to the `alpha`
  prerelease and to a `sha-<12chars>` immutable prerelease.
- **Versioned** (tag push, e.g. `v0.1.0`):
  `agentbox-<version>-<arch>-unknown-linux-musl` and
  `loftd-<version>-<arch>-unknown-linux-gnu` are uploaded to a full
  (non-prerelease) release named after the tag, and to the matching
  `sha-<12chars>` immutable prerelease.
- **Images** (`ghcr.io/<owner>/agentbox:<tag>`,
  `ghcr.io/<owner>/loftd:<tag>`) are published by the image workflow on
  every push to `main` (`latest`, `sha-<12chars>`) and on every tag push
  (the tag name itself, plus `sha-<12chars>`).

 ### Build outputs
- `.#agentbox`: compile from source.
- `.#loftd`: compile the workspace Rust host package with `$out/bin/loftd` as a
  raw dynamic ELF. Runtime helpers are installed under
  `$out/libexec/loftd-helpers`, and the shared `libkrun`/`libkrunfw` packages
  are exposed under `$out/lib/loftd`, so source-built loftd no longer needs a
  wrapper script or duplicate `$out/libexec/loftd` payload.
- `./nix/dev#loftd-dev`: local-checkout-only development build of the
  workspace Rust host package wired to the checked-out `deps/libkrun` and
  `deps/libkrunfw` submodules through the submodule-aware dev flake. Use this
  target for local libkrun/libkrunfw or kernel configuration experiments;
  downstream flakes that consume this repository via `github:` should use
  non-dev root outputs instead. The local firmware build routes C/Kbuild
  compiler calls through `sccache` by default, but repeat-build speedups require
  a persistent `SCCACHE_DIR` or equivalent cache path visible to the Nix build
  sandbox.
- `.#agentbox-prebuilt`: install pinned published binary (currently pinned for
  `x86_64-linux`; use `.#agentbox` elsewhere). This package brings
  `fuse-overlayfs` and `buildah` into the runtime environment for
  `agentbox container` sidecar mode and experimental `agentbox microvm`
  cache misses.
- `.#loftd-prebuilt`: install a pinned published neutral dynamic Linux `loftd`
  asset as raw `$out/bin/loftd`, patch ordinary ELF runtime dependencies with
  Nix, and provide the same package-relative helper and `$out/lib/loftd`
  library layout as source-built `.#loftd`. If a system
  lacks a neutral pinned asset, or is still pinned to a legacy flake-locked
  asset, it fails with a clear diagnostic until a matching neutral `sha-*`
  release is published and pinned.
- `.#agentbox-musl`: static/musl `agentbox`, `agentbox-guest-init`, and
  `loftd-guest-init` binaries for image/guest use. It intentionally does not
  build or expose `bin/loftd`; the host `loftd` binary is always dynamically
  linked so it can load `libkrun.so`/`libkrunfw.so` from the package or dev
  shell runtime library path.
- `.#rmux-prebuilt`: install the pinned published Helvesec/rmux Linux release
  tarball for the current system. The agentbox and loftd images include this
  package as `rmux` alongside Nixpkgs `tmux`.
- `.#rtk-prebuilt`: install the pinned published RTK release asset (currently
  pinned for `x86_64-linux`).
- `.#libkrunfw`: install the pinned `zeroqn/libkrunfw` release asset for the
  current system.
- `.#libkrun`: install the pinned `zeroqn/libkrun` `loftd-*` prebuilt release
  asset for the current system, matching `.#libkrunfw`'s release-asset model.
  Root consumers (`.#crun`, `.#podman`, `.#agentbox`, `.#loftd`, images, and
  `.#loftd-prebuilt`) all use this pinned prebuilt package. The package
  normalizes upstream Linux `lib64` payloads into `$out/lib` and regenerates
  `libkrun.pc` for the Nix store path. Local source development for libkrun is
  intentionally limited to the submodule-aware dev flake (`./nix/dev#loftd-dev`).
- `.#crun`: build `zeroqn/crun` branch `agentbox` with this repo's libkrun
  override, krun handler support, raw data disk annotation support,
  `krun.nested_virt` support, and `pkgs.passt` on crun's runtime `PATH`.
- `.#podman`: build Podman against the custom crun for libkrun/raw-image
  development.
- `.#container-lib-policy-seccomp-json`: install the pinned
  `containers/container-libs` `common/pkg/seccomp/seccomp.json` policy at
  `share/containers/seccomp.json` for downstream flakes or image reuse.
- `.#container`: loftd-compatible Podman image archive named `localhost/loftd:latest`;
  includes rootless Podman tooling such as Podman, Buildah, crun, netavark,
  aardvark-dns, passt, and docker-compose.
- `.#agentbox-container`: agentbox-compatible Podman image archive named
  `localhost/agentbox:latest` for the existing `agentbox` runtime variants;
  includes the same rootless Podman tooling, including Buildah, and Nix
  formatting tooling such as `nixfmt`.

### Nix store / DB diagnostics

`nix build .#container` and `nix build .#agentbox-container` each depend on
a static image metadata linter before running the layered image build command.
To run only those linters:

```bash
nix build .#checks.$(nix eval --raw --impure --expr builtins.currentSystem).container-nix-db-metadata
nix build .#checks.$(nix eval --raw --impure --expr builtins.currentSystem).agentbox-container-nix-db-metadata
```

The check compares store paths referenced by the image Docker config/env against
the `pkgs.closureInfo { rootPaths = layers.imageContents; }` store-path list.
That is the same closure Docker Tools loads into the image Nix DB when
`includeNixDB = true`. It fails fast when image metadata can pull a store path
into `/nix/store` without that path being covered by generated image Nix DB
metadata. This check does not inspect or mutate the host Nix DB.

Inside an agentbox container, run the packaged live DB scanner manually:

```bash
agentbox-nix-store-db-check
```

The runtime checker compares present `/nix/store/<hash>-name` entries with
`nix path-info --all`, ignores the internal `/nix/store/.links` link farm and
transient `*.lock` files, and prints `nix-store --verify-path` evidence for
present-but-invalid paths. When the libkrun Nix disk upperdir is visible at
`/run/agentbox/nix-disk/upper`, failures also compare each invalid store object
with `/run/agentbox/nix-disk/upper/store/<name>` and report whether that
store-layer object is present in the upperdir or not found there. This is
store-layer evidence only, not root-cause proof: absence from the upperdir is
not proof that lower image metadata is correct or that the lower image is at
fault. If `upper/var/nix` or `upper/var/nix/db` exists, the checker reports it
only as metadata-shadow context. It is diagnostic only and never repairs or
mutates the Nix DB.

---

## Quick start

Show CLI help:

```bash
nix develop --command cargo run -p agentbox-host -- --help
```

Build image + loftd binary, then show the loftd CLI:

```bash
nix build .#agentbox-container
podman load < result
nix build .#loftd
./result/bin/loftd --help
```

Image selection behavior:

- default: `localhost/agentbox:latest`
- fallback: `ghcr.io/zeroqn/agentbox:latest`

### Packaged seccomp policy

The image includes the pinned `containers/container-libs` seccomp policy package
and writes global `/etc/containers/containers.conf` with:

```toml
[containers]
seccomp_profile = "/nix/store/...-container-lib-policy-seccomp-json-.../share/containers/seccomp.json"
```

This makes inner Podman use the packaged policy by default while still allowing
per-user containers config to override it. To refresh the policy, update the
`containerLibPolicySeccompJson` revision/hash in `nix/pins.nix`, then rebuild
`.#container-lib-policy-seccomp-json`, `.#container`, and `.#agentbox-container`.

Force GHCR latest:

```bash
./result/bin/agentbox --pull-latest
```

Override image explicitly:

```bash
AGENTBOX_IMAGE=<image-ref> ./result/bin/agentbox
# or
./result/bin/agentbox --image <image-ref>
```

Enable debug logging for troubleshooting agentbox-managed Podman commands:

```bash
./result/bin/agentbox --debug
./result/bin/agentbox container sidecar --debug
```

`--debug` passes `--log-level=debug` to Podman commands that agentbox runs,
including task launch, sidecar setup, image inspection/mounting, health probes,
and cleanup paths. It also allows guest-side diagnostic reports to use stderr.

Collect agentbox component timings:

```bash
./result/bin/agentbox --profile --debug
./result/bin/agentbox container --profile --debug
./result/bin/agentbox microvm --profile --debug
```

`--profile` enables timing collection. Timings are printed only when `--debug`
is also set, and reports are written to stderr so stdout remains reserved for
command output. `--profile` without `--debug` enables measurement but suppresses
reports; `--debug` without `--profile` does not print timing reports. Container
and libkrun task runs emit `agentbox-guest-init` timings. Microvm runs also emit
a host-side `agentbox microvm host profile` report for completed profiled host
phases such as image reference resolution, image cache lookup/ingestion, task
rootfs materialization, guest-init lookup, persistent disk preparation, launch
config build, helper session, task rootfs unmount, and task state cleanup.
Libkrun background Podman prep/wait workers and sidecar debug runs do not emit
guest-init profile reports.
When libkrun `/nix` overlay bootstrap runs, nested
`bootstrap-nix:*` rows break down disk discovery, mount/preseed work, daemon
startup, and the `bootstrap-nix:wait-socket` polling loop.

Enter the final task shell as root when root-only operations are needed:

```bash
./result/bin/agentbox --root
./result/bin/agentbox --root libkrun
./result/bin/agentbox --root container
```

By default, agentbox drops the interactive shell to the host/dev identity.
`--root` is an explicit opt-in that keeps only the final task shell/command as
root inside the guest/container; it does not install or require `sudo`.
Because `--root` is global, `agentbox --root container sidecar` parses, but
sidecar-only mode starts no final task shell so the flag is a harmless no-op
there.

Task containers, including libkrun-backed tasks, are named with the current
repo/workspace slug followed by a unique suffix. For example, a checkout named
`my-repo` appears in `podman ps` as `my-repo-<suffix>`, making active tasks easy
to map back to their repo.

---

## Runtime modes

### 1) Libkrun mode (default)

Run:

```bash
./result/bin/agentbox
./result/bin/agentbox libkrun
./result/bin/agentbox libkrun --mem 8
./result/bin/agentbox libkrun --tsi
./result/bin/agentbox libkrun --publish 127.0.0.1:8080:8080
```

`agentbox` with no subcommand defaults to libkrun. Runtime-specific libkrun
options are accepted under the `libkrun` subcommand.

Inside the image, the configured entrypoint is `agentbox-guest-init default enter --`.
That default guest entrypoint selects the explicit `libkrun` guest
runtime when agentbox passes `AGENTBOX_LIBKRUN_*` environment flags; otherwise
it falls back to the explicit `container` guest runtime. The explicit
`agentbox-guest-init container enter` path does not switch to libkrun.

On first run, agentbox creates two sparse btrfs raw images:

```text
<state-root>/libkrun-nix.raw
<state-root>/libkrun-containers.raw
```

Each default apparent size is `64 GiB`. Because the files are sparse, host disk
usage grows as blocks are written, but guest-visible capacity is still each raw
file's apparent size at VM start.

The raw images are attached with crun annotations:

```text
run.oci.handler=krun
krun.ram_mib=<memory MiB>
krun.cpus=<cpu count>
krun.nested_virt=1
krun.disk.0.path=<state-root>/libkrun-nix.raw
krun.disk.0.id=agentbox-nix
krun.disk.0.readonly=false
krun.disk.1.path=<state-root>/libkrun-containers.raw
krun.disk.1.id=agentbox-containers
krun.disk.1.readonly=false
krun.use_passt=1
--device /dev/net/tun:/dev/net/tun
--publish <publish-spec>
```

By default, agentbox sizes libkrun memory to 80% of host memory, rounded down to
whole GiB, and emits that value with `krun.ram_mib=<MiB>`. Pass
`agentbox libkrun --mem <GiB>` to override it. On Linux, agentbox also emits
`krun.cpus=<n>`: hosts with up to 6 CPUs pass all available CPUs through;
larger hosts reserve 2 CPUs for the host.

Agentbox also emits `krun.nested_virt=1` so crun/libkrun expose VMX/SVM to the
libkrun guest when the host or outer VM already supports nested KVM. This does
not bind-mount host `/dev/kvm` into the guest and cannot enable nested KVM if
the host kernel or outer hypervisor has disabled it. During guest root prep,
`agentbox-guest-init` makes an exposed guest `/dev/kvm` world-accessible so the
default non-root `dev` task shell can use nested KVM.

By default, libkrun mode uses passt networking through `krun.use_passt=1`. Pass
`agentbox libkrun --tsi` to switch to the older TSI/proxy environment path.
Publish inbound ports with repeatable `agentbox libkrun --publish <SPEC>` or
`agentbox libkrun -p <SPEC>`. `<SPEC>` is passed through to Podman using
Podman's publish syntax, for example `8080:80`, `127.0.0.1:8080:80`,
`127.0.0.1::80`, `8080:80/udp`, or `8000-8010:80-90`. Agentbox does not
rewrite the host bind address; include `127.0.0.1:` when the published port
should be loopback-only. Port publishing requires default passt networking and
is rejected with `--tsi`. It applies only to interactive libkrun tasks, not
`resize` or `reset-nix` maintenance runs.

During libkrun guest bootstrap, `agentbox-guest-init` sets
`kernel.dmesg_restrict=1` so kernel logs are root-only inside the guest; the
default `dev` shell cannot read `dmesg`.

For guest-side debugging, test a modified `agentbox-guest-init` without
rebuilding the container image by building only the static guest-init binary and
bind-mounting it over the in-image guest-init path:

```bash
nix build .#agentbox-musl -o result-musl
./result/bin/agentbox libkrun --guest-init ./result-musl/bin/agentbox-guest-init
```

This keeps the normal image entrypoint and shell arguments intact, but the
entrypoint executes the host-provided `agentbox-guest-init` binary. `agentbox`
derives the in-image mount target from the selected image's first entrypoint
element with `podman image inspect`, so this works with the default image,
`--image`, and `AGENTBOX_IMAGE` without a separate target-path environment
variable. The selected image must already be local and inspectable unless you
used an existing path such as `--pull-latest` that pulls it before inspection.

Existing raw images are reused only if `blkid` reports btrfs. Agentbox refuses
to overwrite invalid existing images.

Restart-time btrfs auto-grow is not performed. To grow an existing
agentbox-managed libkrun raw image, use the explicit resize command:

```bash
./result/bin/agentbox libkrun resize --target nix --size 128G
./result/bin/agentbox libkrun resize --target containers --size 128G
```

Targets are limited to the current workspace's managed raw images:

- `nix`: `<state-root>/libkrun-nix.raw`
- `containers`: `<state-root>/libkrun-containers.raw`

Bare integer sizes are interpreted as GiB. Supported binary suffixes include
`G`, `GiB`, `T`, and `TiB`. Resize is grow-only: shrinking and equal-size no-op
requests are rejected. The command validates that the selected managed image
exists, is a regular file, and is btrfs before it extends the sparse raw file.
It then starts a one-shot libkrun guest-init maintenance task to mount the
selected btrfs disk privately under `/run/agentbox/resize-*` and run
`btrfs filesystem resize max`.

Resize launches a direct one-shot `agentbox-guest-init` entrypoint, so the
selected image must be local and inspectable before host-side growth occurs. Use
`--pull-latest` or pre-load/build the image if needed.

The resize command refuses to run if Podman reports a running container with a
matching `krun.disk.*.path` annotation, and it fails closed if that live-state
probe cannot complete. It does not live-resize active disks, does not auto-grow
during normal `agentbox libkrun` startup, does not accept arbitrary image paths,
and does not reset or migrate state. Avoid starting a libkrun task concurrently
with resize; the live-state check cannot eliminate every race between the probe
and the one-shot maintenance task.

If the host raw file is enlarged but the one-shot guest filesystem resize fails,
agentbox reports the failure as retryable. Fix the reported guest issue and
rerun the same resize command; agentbox will not shrink, roll back, or reset the
raw image automatically. Full end-to-end verification requires a real libkrun
guest with the raw disk mounted, so this path should be manually smoke-tested in
addition to the host and guest unit tests.

To discard the current workspace's managed libkrun `/nix` disk and recreate it
at the default size, use the explicit reset command:

```bash
./result/bin/agentbox libkrun reset-nix --force
```

`reset-nix` only targets `<state-root>/libkrun-nix.raw`; it does not reset the
containers raw image, run any guest VM maintenance step, migrate state, create a
backup, or prompt interactively. `--force` is required: without it the command
fails before probing Podman or touching the filesystem. With `--force`, agentbox
first refuses to proceed if Podman reports any running container with a matching
`krun.disk.*.path` annotation, then deletes the existing managed `/nix` raw file
if present and creates a fresh default btrfs image. Non-file paths at the managed
image location are refused instead of removed.

No live auto-resize, state migration, snapshot/rollback UX, direct microvm
host-port helper UX, rootful nested Podman workflow, or container-mode
nested-Podman support is implemented.

Manual host smoke checklist for the nested rootless Podman runtime:

1. Build and load `.#container`, then start default libkrun mode on the host.
2. Inside the guest, confirm the shell is `dev` and run `podman info`; the
   `podman` compatibility command waits for rootless Podman prep to finish,
   then execs real Podman. Verify rootless mode and storage driver `btrfs`.
3. Confirm the Docker-compatible API endpoint is exported:

   ```bash
   echo "$DOCKER_HOST"
   ```

   `DOCKER_HOST` should be `unix:///run/user/<uid>/podman/podman.sock`. The
   socket is created lazily by Docker-compatible commands rather than by guest
   shell entry or direct `podman`.
4. Run `docker info`; it should use the Docker compatibility wrapper, wait for
   Podman prep if needed, start or repair the rootless Podman API socket, and
   report the same rootless Podman storage instead of starting `dockerd`.
   After this, the socket should exist and accept remote Podman requests:

   ```bash
   test -S "$XDG_RUNTIME_DIR/podman/podman.sock"
   podman --remote --url "$DOCKER_HOST" info
   ```

   The socket is a rootless Podman API endpoint for the `dev` user. It is less
   privileged than a rootful Docker daemon socket, but still grants API-level
   control over that user's containers and images, so treat it as a trusted
   in-guest development interface.
5. Confirm `/dev/net/tun` exists inside the guest, then run both:

   ```bash
   podman run --rm docker.io/library/alpine:latest echo hello
   docker run --rm docker.io/library/alpine:latest echo hello
   docker-compose version
   ```

6. Exit and restart agentbox; verify pulled Podman image/storage persists via
   `<state-root>/libkrun-containers.raw`. Runtime state should live under
   `/home/dev/.local/share/containers/storage`; `/var/lib/docker`,
   `/var/lib/containerd`, and `/home/dev/.local/share/docker` should be absent.
7. For Podman troubleshooting, inspect `/run/agentbox/podman-prep.status`,
   `/run/agentbox/podman-prep.log`, and
   `$XDG_RUNTIME_DIR/podman/podman.sock`. Use
   `agentbox-guest-init libkrun podman wait` to wait only for prep readiness,
   or `agentbox-guest-init libkrun podman service-wait` to also start/repair
   the Docker-compatible Podman API socket.
8. Confirm no fuse-overlayfs path/config/binary is required by rootless Podman
   setup.

Libkrun mode intentionally does **not** use the container sidecar/overlay bridge,
does **not** set `AGENTBOX_NIX_PROXY_HOST`, does **not** fall back to seeded Nix
state, and does **not** use fuse-overlayfs for nested rootless Podman storage.

---

### 2) Container mode

Run:

```bash
./result/bin/agentbox container
```

What container mode does (high level):

1. Resolves the selected image and mounts its filesystem.
2. Uses image `/nix` as `lowerdir` for host `fuse-overlayfs`.
3. Builds an external merged Nix tree under project state.
4. Starts/reuses a deterministic native Podman `nix-daemon` sidecar daemon.
5. Preserves that sidecar while matching task containers are still running.
6. Starts the interactive container with read-only `/nix` + daemon socket.
7. When the last matching task container exits, removes the idle sidecar and
   unmounts the `nix-merged` FUSE overlay in the matching Podman mount
   namespace so `fuse-overlayfs` does not linger.

Overlay writes live in `<state-root>/nix-upper`; `nix-merged` is only the
mounted merged view and may be unmounted/recreated between runs.

Sidecar metadata is saved at:

```text
<state-root>/nix-sidecar.state
```

New sidecar metadata is container-only. Legacy metadata from older libkrun/TSI
sidecar experiments is tolerated for safe cleanup/recreate decisions, but it is
not reused as the current native sidecar configuration while matching legacy task
containers are still active.

Container mode always requires the managed sidecar. No direct/no-sidecar
container mode is currently implemented.

#### Sidecar debugging

Start or reuse just the container nix-daemon sidecar stack, print the sidecar
name and host proxy port, and exit without launching the interactive task
container:

```bash
./result/bin/agentbox container sidecar
```

`agentbox container sidecar` intentionally leaves the sidecar container and
merged nix overlay running after exit so they can be inspected. It skips the
nix-daemon socket health probe so a broken daemon can still be debugged after
container startup.

Use the printed sidecar name for inspection and cleanup, for example:

```bash
podman logs <sidecar-name>
podman port <sidecar-name> 19876
podman rm -f <sidecar-name>
```

---

### 3) Microvm mode (experimental)

Run/help:

```bash
./result/bin/agentbox microvm --help
./result/bin/agentbox microvm --storage auto
./result/bin/agentbox microvm --storage btrfs-snapshot
./result/bin/agentbox microvm --storage fuse-overlay
./result/bin/agentbox microvm --storage reflink
```

`agentbox microvm` is an explicit experimental task-based direct-libkrun runtime.
It is intentionally separate from the current Podman-backed `libkrun` mode. The
microvm path does not call the Podman-backed image resolver, `podman run`,
`crun`, or `runc`; cache misses require the microvm-owned Buildah ingestion
path, and `agentbox --pull-latest microvm` currently fails clearly instead of
reusing Podman semantics.

Current implemented milestone:

- image references are resolved against a global per-user immutable cache under
  the agentbox state root;
- digest-pinned references hit by digest;
- mutable tags may hit only through local ref-to-digest metadata, which is a
  cache hint rather than an authoritative freshness check;
- cache misses are ingested through the microvm-owned rootless Buildah path:
  one `buildah unshare` transaction runs the hidden Rust ingestion child, which
  performs `buildah from`, `buildah inspect --format '{{.FromImageDigest}}'`,
  `buildah mount`, opportunistically creates a btrfs subvolume cache rootfs,
  copies into the digest-keyed cache with `cp -a --reflink=auto`, validates
  exact-one executable `agentbox-guest-init`, atomically finalizes the
  compatibility marker, then performs explicit mount/container/staging cleanup;
- per-task writable rootfs directories are materialized from compatible cached
  roots through explicit copy-on-write storage methods. `btrfs-snapshot` uses a
  writable btrfs subvolume snapshot and deletes it through `buildah unshare`;
  rootless deletion requires the backing btrfs mount option
  `user_subvol_rm_allowed`. `fuse-overlay` mounts a real `fuse-overlayfs`
  merged root from cached lower plus per-task upper/work dirs, and explicit
  `reflink` requires `cp -a --reflink=always`. Plain recursive task-rootfs
  copies are not used. Normal cleanup deletes task subvolumes, unmounts any task
  overlay, and removes the task state dir; `--preserve-debug` intentionally
  preserves the task rootfs for inspection;
- the parent process prepares two per-workspace sparse btrfs raw disks:

  ```text
  <state-root>/microvm-nix.raw
  <state-root>/microvm-containers.raw
  ```

  Each disk has a default apparent size of `64 GiB` and is reused only when the
  existing file validates as btrfs. Invalid existing files are refused rather
  than reformatted automatically;
- the parent process writes a std-only `KEY=hex-encoded-value` launch config,
  starts a hidden same-binary helper, waits for the helper status, maps that
  status back to the `agentbox microvm` exit code, and still cleans the task
  rootfs after helper failure;
- the helper dynamically loads `libkrun.so` using `AGENTBOX_LIBKRUN_LIBRARY`
  when set, otherwise by normal soname lookup. The Nix package supplies the
  libkrun/libkrunfw library path for the normal packaged path. After loading, it
  creates a context, sets CPU/memory, sets the task rootfs, attaches `/nix` and
  container-store disks through `krun_add_disk`, disables libkrun's implicit
  console, wires stdio through `krun_add_virtio_console_default(0, 1, 2)`, adds
  the workspace virtiofs device, sets workdir/exec and env through
  `krun_set_exec`, then calls `krun_start_enter()`;
- inside the guest, `agentbox-guest-init microvm enter` mounts the workspace
  virtiofs tag at `/workspace`, starts reusable Nix and rootless container-store
  preparation against the attached disks, then drops to the host/dev identity and
  execs the default `fish -l` shell.

Current limitations: direct microvm inbound port publishing is intentionally
unavailable, and real VM smoke validation is still pending. See
`docs/microvm-smoke.md` for the manual smoke checklist and current not-tested
items.

The storage policy values are:

- `auto`: try `btrfs-snapshot` first, then clean partial output and try the
  `fuse-overlayfs` fallback. `auto` does not select `reflink`.
- `btrfs-snapshot`: require `buildah unshare btrfs subvolume snapshot` for
  task-rootfs materialization and `buildah unshare btrfs subvolume delete` for
  cleanup; fail instead of falling back to another backend. Rootless cleanup
  requires the backing btrfs mount to include `user_subvol_rm_allowed`; if
  cleanup reports `Operation not permitted`, inspect the mount with
  `findmnt -T '<task-rootfs>' -o TARGET,SOURCE,FSTYPE,OPTIONS`, add
  `user_subvol_rm_allowed` to the matching `/etc/fstab` btrfs entry, remount
  with `sudo mount -o remount,user_subvol_rm_allowed <mountpoint>`, and retry
  `buildah unshare btrfs subvolume delete '<task-rootfs>'`. Existing
  non-subvolume cache entries may need refresh before this explicit mode works.
- `fuse-overlay`: require the portable `fuse-overlayfs` image-rootfs path with
  cached image rootfs as lowerdir and per-task upper/work/merged dirs.
- `reflink`: explicit opt-in only; require `cp -a --reflink=always` for
  task-rootfs materialization and fail instead of falling back to byte-for-byte
  file copies.

The shorthand `btrfs` storage policy remains intentionally rejected; use the
precise `btrfs-snapshot` policy for snapshot-backed task roots.

Image selection remains global through `--image` or `AGENTBOX_IMAGE`; microvm
does not add a runtime-local image flag.

Failure diagnostics are phase-classified around cache ingestion, storage backend
selection, task rootfs materialization, guest-init resolution, persistent disk
preparation, launch config construction, helper/libkrun launch, task rootfs
unmount, and task state cleanup. If `--preserve-debug` is set and a failure
happens after task rootfs materialization, the error reports the preserved task
rootfs, task state directory, expected `launch.conf` path, and for fuse-overlay
tasks an explicit unmount hint for later cleanup. Preserved btrfs-snapshot
tasks report the matching `buildah unshare btrfs subvolume delete` cleanup
command and point permission-denied cleanup failures at the btrfs
`user_subvol_rm_allowed` mount option.

---

### 4) Loftd extraction (Phase 4 complete)

`loftd` is the extracted direct-libkrun microvm runtime owner. Phase 4 is
complete: the implementation builds a typed launch plan, uses Buildah as the
durable OCI image source for the default btrfs path, materializes a per-task
btrfs snapshot rootfs, prepares loftd-owned persistent raw btrfs disks for
`/nix` and rootless container storage, starts a same-binary helper through a
strict keep-id `unshare` wrapper around `<loftd-exe> internal
libkrun-network-enter <launch.conf>` to set up the per-session pasta namespace
and call libkrun, and enters the guest through `loftd-guest-init enter`.
Interactive loftd runs are managed by a guest-side PTY session manager, so the
host terminal is an attach client rather than the lifetime owner of the guest
shell or terminal command. The helper owns final cleanup for managed sessions;
`loftd kill` remains the recovery path for detached tasks, and
`--preserve-debug` keeps task state for manual inspection. Managed attach
sockets are runtime-only host sockets under `/tmp/loftd-<uid>/`; the active-task
record stores the exact socket path for `loftd attach`, and helper cleanup
removes only the current task's socket. The explicit `fuse-overlay` backend is
still a future slice.

Run/help:

```bash
./result/bin/loftd --help
./result/bin/loftd --rootfs-backend btrfs-snapshot
./result/bin/loftd --rootfs-backend fuse-overlay
./result/bin/loftd --pull-latest
./result/bin/loftd --image ghcr.io/example/loftd:dev
./result/bin/loftd --daemon
./result/bin/loftd --landlock=all -- bash -lc 'echo ok'
./result/bin/loftd --landlock=best-effort -- bash -lc 'echo ok'
./result/bin/loftd --landlock=off -- bash -lc 'echo ok'
./result/bin/loftd --seccomp=off -- bash -lc 'echo ok'
./result/bin/loftd --seccomp=audit:loftd-seccomp.trace.jsonl -- bash -lc 'echo ok'
./result/bin/loftd seccomp synthesize --input loftd-seccomp.trace.jsonl --output loftd-seccomp.policy.json
./result/bin/loftd --seccomp=audit:loftd-seccomp.policy.json:loftd-seccomp.denied.jsonl -- bash -lc 'echo ok'
./result/bin/loftd --seccomp=audit-default:loftd-seccomp.denied.jsonl -- bash -lc 'echo ok'
./result/bin/loftd seccomp extend --policy loftd-seccomp.policy.json --trace loftd-seccomp.denied.jsonl --output loftd-seccomp.updated.json
./result/bin/loftd seccomp extend --default-policy --trace loftd-seccomp.denied.jsonl --output loftd-seccomp.updated.json
./result/bin/loftd --seccomp=enforce:loftd-seccomp.updated.json -- bash -lc 'echo ok'
./result/bin/loftd --passt -- bash -lc 'echo ok'
./result/bin/loftd --profile -- bash -lc 'echo ok'
./result/bin/loftd --guest-init ./result-musl/bin/loftd-guest-init -- bash -lc 'echo ok'
./result/bin/loftd -- bash -lc 'echo ok'
./result/bin/loftd ps
./result/bin/loftd attach <task-id-or-handle-selector>
./result/bin/loftd a <task-id-or-handle-selector>
./result/bin/loftd kill <task-id-or-handle-selector>
./result/bin/loftd container-store resize --size 128G
./result/bin/loftd container-store reset --force
```


Detach/attach behavior:

- A normal foreground `loftd` run starts a managed guest PTY session and then
  attaches the host terminal to it. The foreground experience is still an
  interactive shell or command, but the guest process is not tied to the host
  terminal lifetime. Managed guest PTY sessions preserve the launching host
  terminal identity by passing non-empty UTF-8 `TERM`, `COLORTERM`,
  `TERM_PROGRAM`, and `TERM_PROGRAM_VERSION` values into the guest; this is
  limited to the managed attach path and does not enable broad host environment
  passthrough. The guest init also defaults missing or empty `LANG` and
  `LC_CTYPE` to `C.UTF-8` so locale-sensitive terminal programs such as `tmux`
  can use UTF-8 character widths, including CJK text. Explicit locale values
  are preserved, `LC_ALL` is not set, and this does not broaden host
  environment passthrough.
- Press `Ctrl-\` twice to detach from the current terminal
  session. loftd recognizes both raw `Ctrl-\` bytes and CSI-u/Kitty-encoded
  `Ctrl-\` events from terminals or multiplexers. The host-side filter
  intercepts the sequence before it reaches the guest TTY, where `Ctrl-\`
  would otherwise be the POSIX quit character. Closing the host terminal,
  killing the attach client, or losing SSH also behaves as detach: the guest
  shell or terminal-interactive command keeps running while the VM helper
  remains active.
- `loftd --daemon` starts the managed guest PTY through the launching terminal,
  forwards startup input/output so the target program can complete terminal
  initialization, then detaches automatically after the first target output is
  followed by a short idle window. The heuristic is generic and does not parse
  shell prompts. This mode is TTY-only; if stdin or stdout is not a terminal,
  loftd fails before sending the attach frame that starts the target program.
  Use `loftd attach <task-id-or-handle-selector>` (or
  `loftd a <task-id-or-handle-selector>`) to reconnect and
  `loftd kill <task-id-or-handle-selector>` to terminate the detached task.
- Reconnect with `loftd attach <task-id-or-handle-selector>` or its `loftd a`
  shortcut. Selectors follow the same task-id/handle matching rules as
  `loftd kill`; use `loftd ps` to list running task IDs and handles. Reattach
  repaints the current visible terminal screen from bounded in-memory guest PTY
  state before forwarding new output, so a detached shell or TUI should be
  usable without pressing `Enter` just to
  redraw. This restore state is not persisted across helper or VM restart.
- Only one attach client is supported at a time. A second attach attempt receives
  a busy error instead of sharing the PTY.
- Terminal and TUI programs inside the PTY are in scope, including
  `loftd -- <interactive-command>`. Graphical X11/Wayland application
  preservation is not implemented; display sockets and GUI reconnect semantics
  need a separate design.
- The attach transport is libkrun's vsock-to-host-Unix-socket mapping. If the
  required `krun_add_vsock_port2` symbol or setup path is unavailable, managed
  attach fails clearly instead of falling back to another transport.
- Exiting the guest shell or command terminates the VM and removes the active
  task/rootfs unless `--preserve-debug` was used. Detached tasks can be
  terminated with `loftd kill <task-id-or-handle-selector>`.


Landlock behavior:

- Host-side loftd Landlock is applied to the libkrun VM-worker process after
  prepared-root and libkrun setup that require broader host access, but before
  `krun_start_enter`. It is applied before seccomp so the Landlock syscalls are
  not blocked by the seccomp filter.
- For ordinary task launches, omitting `--landlock` is equivalent to
  `--landlock=relax`. Relax mode is fail-closed for the non-network Landlock
  feature families loftd handles, including filesystem access rules, device
  ioctl access handling, IPC scopes for abstract UNIX sockets and signals, and
  audit-flag support. It intentionally does not handle TCP `BindTcp`, so
  guest-local listeners such as websocket or dev-server ports can bind inside
  the guest without disabling the rest of loftd's host-side Landlock layer.
- `--landlock=all` preserves the stricter TCP bind behavior: loftd additionally
  handles TCP `BindTcp` and constrains it to simple published TCP host ports when
  they are known.
- `--landlock=best-effort` uses the `relax` policy shape, including unrestricted
  TCP `BindTcp`, but applies only the supported subset and logs the effective
  policy plus any non-fully-enforced status. This is the explicit compatibility
  path for older kernels or hosts with partial Landlock support.
- `--landlock=off` disables only this host-side Landlock layer. It does not
  disable the default host-side seccomp policy; use `--seccomp=off` separately
  if you need to debug seccomp.
- The first cut confines the VM worker and its future children only. It does not
  claim to confine the guest kernel, guest Podman, the keep-id helper before the
  VM worker, or network manager/pasta/passt processes started before the VM
  worker.
- Filesystem rules are derived from the launch config: the prepared root is
  read/execute only, declared read-write bind mounts and disks are writable,
  declared read-only bind mounts remain read-only, and host `/nix` overlay paths
  are categorized by lower/upper/work/merged role. If a broader writable parent
  rule is required for profiling output, the effective-policy report labels
  affected read-only children as mount-enforced instead of Landlock-enforced.
- TCP `ConnectTcp` is intentionally unrestricted by this first cut to preserve
  existing guest/network behavior. Landlock's connect rules are per remote TCP
  port, and loftd does not yet have an outbound allowlist. TCP `BindTcp` is
  unrestricted in `relax` and `best-effort`; it is handled and constrained to
  simple published TCP host ports only in `all`.
- Guest-local binds do not expose host ports by themselves. Host inbound
  exposure remains controlled by repeatable `-p, --publish SPEC`; without a
  publish rule, a process may bind inside the guest VM but incoming host
  connections are not forwarded to it.
- Before restriction, loftd inventories retained file descriptors. Fail-closed
  modes (`relax` and `all`) fail on unexpected retained regular files because
  descriptors opened before Landlock can retain access outside the filesystem
  rules.
- The effective-policy report is emitted in debug logs and includes mode, path
  categories/access classes, whether BindTcp is unrestricted or restricted to
  published ports, the explicit `ConnectTcp` unrestricted-by-design marker, IPC
  scopes, audit flags, and retained-FD classifications.

Seccomp behavior:

- Host-side loftd seccomp is incubating. For ordinary task launches, omitting
  `--seccomp` makes loftd enforce the packaged default policy at
  `$out/share/loftd/seccomp/default.json`. This is fail-closed: if the packaged
  policy is missing, unreadable, invalid, or cannot be compiled for the host
  architecture, the launch fails before the VM worker enters libkrun.
- `--seccomp=off` is the explicit no-filter spelling and opt-out for a normal
  task launch. Maintenance/internal one-shot VMs such as
  `loftd container-store resize/reset` remain default-off for this milestone.
- `--seccomp=audit:<trace>` (also accepted as `--seccomp=trace:<trace>`) runs
  the libkrun VM-worker entrypoint under `strace -f`, writes a tracer-owned raw
  log, and converts it to the requested JSONL trace when the helper observes
  the VM worker exit. The raw `.strace` sidecar can include VM-worker setup
  and cleanup syscalls; the finalized JSONL starts after the internal start
  marker emitted immediately before `krun_start_enter` and then keeps only
  syscall lines from the traced PID that emitted that marker plus post-marker
  descendants linked by observed `clone3`, `clone`, `fork`, or `vfork` returns.
  This excludes unrelated parent cleanup syscalls such as
  post-VM unmounts from policy synthesis input while preserving the raw sidecar
  for diagnostics. Missing the start marker or its traced PID fails trace
  finalization instead of publishing an unscoped JSONL trace. The keep-id helper
  setup, including `newuidmap` and `newgidmap`, is not traced. Use the raw
  `.strace` sidecar only for debugging.
- `loftd seccomp synthesize --input <trace> --output <policy>` extracts syscall
  names from the trace and writes a deterministic `seccompiler` JSON policy with
  a `main_thread` allowlist.
- `--seccomp=audit:<policy>:<denied-trace>` (also accepted as
  `--seccomp=trace:<policy>:<denied-trace>`) is a policy-aware gap audit. It
  still runs without installing a seccomp filter, but asks `strace` to record
  only syscall names that are not already listed in
  `<policy>`'s `main_thread.filter[*].syscall` allowlist. The raw gap sidecar
  still keeps the audit marker and `clone3`/`clone`/`fork`/`vfork` lines visible
  so finalization can reconstruct the VM-worker lineage even when those syscalls
  are already allowed. The resulting `<denied-trace>` JSONL uses the same
  lineage-scoped trace record shape as full audit, but remains missing-only by
  suppressing baseline-allowed lineage bookkeeping records during finalization.
  "Denied" here means "observed by strace but missing from the baseline policy";
  it does not mean a kernel seccomp denial occurred.
- `--seccomp=audit-default:<denied-trace>` (also accepted as
  `--seccomp=trace-default:<denied-trace>`) is the same gap audit against the
  packaged default policy at `$out/share/loftd/seccomp/default.json`, without
  spelling that policy path. This is also fail-closed: if the packaged default
  policy is unavailable or invalid, loftd fails before launching the traced VM
  worker instead of falling back to full audit.
- `loftd seccomp extend --policy <baseline> --trace <denied-trace> --output
  <updated-policy>` additively appends missing syscall allow rules from a full
  or gap audit trace to an existing policy. Use `--default-policy` instead of
  `--policy <baseline>` to extend from the packaged default policy without
  spelling its path; exactly one of `--policy` or `--default-policy` is required.
  It preserves existing filter entries and appends new syscall-only entries in
  deterministic syscall-name order. The output is validated with `seccompiler`
  before loftd writes it; the baseline policy file is not modified.
- `--seccomp=enforce:<policy>` loads that `seccompiler` JSON policy and
  installs it in the VM worker immediately before `krun_start_enter`. Passing an
  explicit enforce path overrides the packaged default policy for that run.
- Gap audit is a debugging aid, not proof that enforcement is safe. It compares
  syscall names only; it does not diff or prove seccompiler argument-condition
  rules. Always test the updated policy explicitly with
  `--seccomp=enforce:<policy>`.
- This is loftd host-helper filtering only. It does not change guest Podman's
  seccomp profile.
- On NixOS hosts where audit mode fails with ptrace errors such as
  `PTRACE_TRACEME: Operation not permitted`, first check:

  ```bash
  sysctl kernel.yama.ptrace_scope
  ```

  `kernel.yama.ptrace_scope=1` normally allows tracing a direct child, which is
  the audit-mode workflow. Only hosts that disable ptrace more broadly should
  need a temporary host-policy change such as:

  ```bash
  sudo sysctl kernel.yama.ptrace_scope=0
  ```

  Persisting any ptrace relaxation is a host policy decision, commonly
  represented with `boot.kernel.sysctl."kernel.yama.ptrace_scope"` in NixOS
  configuration.

Container-store disk maintenance:

```bash
./result/bin/loftd container-store resize --size 128G
./result/bin/loftd container-store reset --force
```

These commands manage only the current workspace's `loftd-containers.raw` disk
used by the raw-disk container store. They do not inspect or migrate any legacy
host-directory container store and do not resize or reset loftd's host `/nix`
overlay state.
`resize` is grow-only: `--size` accepts bytes or binary suffixes such as `K`,
`M`, `G`, `T`, `KiB`, `MiB`, `GiB`, and `TiB`, and the requested size must be
larger than the current raw file. It grows the host sparse file first, then
starts a narrow one-shot direct-libkrun maintenance VM that runs
`loftd-guest-init internal resize containers` to expand the guest btrfs
filesystem. If that guest resize fails after the host file has grown, loftd does
not shrink or roll back the file; fix the reported VM/guest problem and rerun
the same resize command.

`reset` is destructive and requires `--force`. It refuses to run while the
current workspace has running, pid-reused, unreadable, or unscannable task
records, deletes an existing regular `loftd-containers.raw`, and recreates the
default 64 GiB sparse btrfs image without launching a VM. Stale-only task
records are reported as cleanup information and do not block either command.
For a manual smoke test on a host with Buildah, btrfs-progs, and libkrun
available, run `loftd --container-store raw-disk` once, then run the `resize`
and `reset --force` commands above.

When `--mem` is omitted, loftd now sizes the direct-libkrun VM to 80% of host
memory rounded down to whole GiB, matching agentbox libkrun mode. Pass
`loftd --mem <GiB>` to override that default. Guest bootstrap also sets
`SCCACHE_DIR=/home/dev/.cache/sccache`, backed by loftd's shared state
`sccache` bind mount.

Root shell handoff:

```bash
./result/bin/loftd --root
# inside the root shell:
loftd-as-dev          # execs fish -l as dev
loftd-as-dev id -un  # runs a command as dev
```

`loftd-as-dev` is packaged only in the loftd image. It is a narrow root-only
helper for dropping from an interactive loftd root shell back to the materialized
`dev` account. With no arguments it launches `fish -l`; with arguments it runs
that command as `dev`. Exiting that fish or command returns to the invoking root
shell only when the helper was started as a child process from an interactive
root shell. The helper does not provide sudo/su, does not switch arbitrary users,
and cannot be used by `dev` to regain root.

For host-side and direct-libkrun diagnostics, use `--log-level` with one of
`off`, `error`, `warn`, `info`, `debug`, or `trace`. The same effective level is
used by the parent process, the keep-id libkrun helper, and libkrun logging;
`debug` and `trace` also set `LOFTD_GUEST_DEBUG=1` so `loftd-guest-init` prints
early guest-entry breadcrumbs to stderr. `LOFTD_LOG_LEVEL` provides the same
setting through the environment. When neither `--log-level` nor
`LOFTD_LOG_LEVEL` is set, `--debug` remains accepted as a compatibility alias
for `--log-level debug`; otherwise a scalar/global `RUST_LOG` value such as
`debug` or `trace` can enable loftd tracing. Target-specific `RUST_LOG` filters
still drive Rust tracing, but are not guessed into a libkrun numeric level.

For timing diagnostics, `loftd --profile` emits `loftd host profile` and
`loftd-guest-init profile` reports to stderr for completed btrfs-snapshot host
and guest-init phases such as launch-plan build, task rootfs materialization,
persistent disk preparation, guest-init lookup, launch config build, helper
session, task state cleanup, and early guest bootstrap. Btrfs rootfs profile
metadata includes `task_rootfs_cache_status` (`hit`, `miss-populated`,
`miss-rebuilt`, or `direct-uncached`), `task_rootfs_cache_digest_key` when a
known digest keys the cache entry, optional `task_rootfs_cache_path`, and
`task_rootfs_cache_uncached_reason` for direct uncached runs. The
`task_rootfs_materialization` row remains the aggregate rootfs phase; when
profiling is enabled, subordinate rows such as
`task_rootfs_materialization:reset_task_dir`,
`task_rootfs_materialization:buildah_version`,
`task_rootfs_materialization:select_image_attempt`,
`task_rootfs_materialization:resolve_image_digest`,
`task_rootfs_materialization:cache_entry_read`,
`task_rootfs_materialization:cache_snapshot`,
`task_rootfs_materialization:buildah_materializer`, and
`task_rootfs_materialization:cache_population` show the host-side path that ran.
Cache-hit runs usually stop at `cache_snapshot`, direct-uncached runs skip cache
population, and `buildah_materializer` intentionally treats the Buildah
unshare child as a black box. These detail rows are diagnostics for the path
taken and should not be treated as an additive replacement for the aggregate
row. The host report keeps
the aggregate `helper_session` row and, when profiling is enabled, also emits
scoped helper/VM-worker host reports with `profile_scope` metadata for the
helper command build/spawn/wait path, helper setup, passt handoff, VM-worker
fork/wait, prepared-root setup, libkrun open, libkrun pre-enter configuration,
and the blocking libkrun guest session when control returns to Rust. The helper
report also imports VM-worker child phase timings under
`helper_wait_vm_worker_child_*` rows from a pre-handoff artifact written before
`krun_start_enter`. The vendored libkrun build appends opt-in internal
`libkrun_*` TSV rows to that same artifact through `krun_set_profile_path`.
loftd prints those rows as a separate `libkrun profile` section with raw
nanosecond (`ns`) values plus a derived millisecond rendering, instead of
merging them into loftd's millisecond host profile rows. The libkrun section can
show event-manager creation, context take, firmware/block/kernel-cmdline/net/
vsock/gpu-console/identity setup, and selected microVM build phases such as
payload choice, guest-memory creation, vCPU start, and event-subscriber
registration. `helper_wait_vm_worker_child_unattributed` covers any remaining
wait time outside the known loftd-owned child setup phases, usually guest
runtime or libkrun event-loop time after the handoff. `--profile` does not raise
loftd, guest-init, or libkrun debug logging;
use `--log-level debug`, `--log-level trace`, or the compatibility form
`--debug` separately when verbose diagnostic logs are needed. Stdout remains
reserved for guest command output.

For managed PTY attach-loop latency diagnostics, set
`LOFTD_ATTACH_PROFILE=1` when launching `loftd`. This is separate from
`--profile`: it records interactive attach hot-path counters rather than startup
and lifecycle phases. When enabled at launch time, loftd propagates the flag to
`loftd-guest-init` and both sides emit one `loftd attach profile` summary line
to stderr on detach or exit. The host summary includes frame-read, payload size,
stdout write, and stdout flush timings; the guest summary includes PTY readable
events, PTY read sizes, full-buffer read count, terminal normalize/parser time,
and guest frame-write time. Guest summaries keep the compatibility
`normalize_parse_total_us` and `normalize_parse_max_us` fields as exact per-read
combined terminal-processing timings, and also include split `normalize_*` and
`parser_*` fields for new latency analysis. Attaching to an already-running
managed task profiles the host attach path immediately, but guest-side attach
metrics are available only if that task was originally launched with
`LOFTD_ATTACH_PROFILE=1`.

Loftd troubleshooting FAQ:

- If the interactive shell appears to hang during startup, check the host
  `RLIMIT_NOFILE` limits inherited by the process that launched loftd:

  ```bash
  ulimit -Sn
  ulimit -Hn
  ```

  Loftd raises the helper's soft `nofile` limit to the inherited hard limit
  before starting libkrun, then asks libkrun to set the guest VM's
  `RLIMIT_NOFILE` soft and hard limits to that same inherited hard limit. It
  cannot raise above the parent launcher's hard limit. If `ulimit -Hn` is low,
  raise the hard limit in the actual parent launcher context first, such as the
  shell, tmux session, systemd unit, or service that starts `loftd`, then start
  loftd again from that context. Loftd treats guest nofile setup as required:
  startup fails if the loaded libkrun does not provide `krun_set_rlimits` or
  rejects the nofile limit request.

Active task control is loftd-native and does not use host Podman as a runtime
backend:

```bash
loftd ps
loftd kill <task-id-or-handle-selector>
```

`loftd ps` scans loftd's app state and lists active task VM records across all
workspaces by default. The human-readable table includes a short handle, full task
id, status, helper PID/session identity, start timestamp, image, and workspace
slug. For a full task id like `agentbox-4138-178109091122334455`, the handle is
`agentbox-4138`, so `loftd kill agentbox-4138` can target that task without
typing the opaque suffix. You can also use a displayed-handle prefix of at least
two characters, such as `loftd kill ag`, when that prefix uniquely matches one
visible handle. For handles shaped like `<name>-<number>`, `loftd kill` also
accepts `<name-prefix>-<handle-number-prefix>` when it uniquely matches a
displayed handle; for example, `loftd kill ag-18` can target
`agentbox-1845-<opaque>` through its displayed handle `agentbox-1845`. The
handle-number prefix is the numeric segment shown in the displayed handle, not
the helper process PID. Prefix matching is only against displayed handles, not
full task ids. It is an active-task view only: completed task history, log
inspection, JSON/API output, restart/pause/exec operations, and Podman-backed
management are intentionally out of scope.
`loftd kill <task-id-or-handle-selector>` validates the recorded process and
session identity before signaling the task process group, sends `SIGTERM`, waits
briefly, and escalates to `SIGKILL` only if the task is still running. Ambiguous
handles or handle selectors, too-short prefixes, malformed abbreviated
selectors, reused process ids, or unreadable process identities are reported
instead of signaled. Stale records for already-exited tasks are eligible for a
cleanup retry without signaling. A successful kill request only returns after
the task rootfs/state cleanup succeeds, then removes the active record from
subsequent `ps` output. If cleanup fails after the VM process is gone, `loftd
kill` returns a visible error and leaves or restores the active record so rerun
`loftd kill <task-id-or-handle-selector>` can retry the same cleanup.

To inspect a preserved task `launch.conf`, decode its internal hex line format:

```bash
loftd decode-launch-conf <task-state-dir>/launch.conf
```

The decoder prints `KEY=decoded-value` lines with control characters escaped for
readability. It is a debugging aid for files preserved through `--preserve-debug`;
the launch path still consumes the encoded private handoff format.

Image selection is materialized through Buildah for the btrfs-snapshot path: with no
image option, loftd first inspects `localhost/loftd:latest` and uses it with
`--pull=never` when present, otherwise loftd uses `ghcr.io/zeroqn/loftd:latest`
with `--pull=missing`. The flake's canonical `.#container` output builds that
local `localhost/loftd:latest` image with `loftd-guest-init enter` as its guest
contract. `--pull-latest` refreshes the canonical image through Buildah before
cache lookup, and `--image` uses exactly the supplied image reference with
`--pull=missing`. `--image` and `--pull-latest` are mutually exclusive.

Loftd also exposes a local image-cache management surface:

```bash
loftd images list
loftd images sync ghcr.io/example/loftd:dev
loftd images sync ba5a514
loftd images remove --dry-run feedfacecafe
loftd images remove feedfacecafe
loftd images remove ghcr.io/example/loftd:d
```

`loftd images list` is read-only and reports a Buildah-aligned table with
`REPOSITORY`, `TAG`, short `IMAGE ID`, short `DIGEST`, `CACHE`, `BUILDAH`, and
`PATH` columns. Cached rows remain digest-keyed internally, but the default view
omits the redundant digest key and shows about twelve digest/image-id characters
for copyable selectors. Buildah inventory rows that do not have a matching loftd
cache entry are included as `CACHE=uncached` and `BUILDAH=local-only`; old or
untagged local Buildah rows preserve Buildah's literal `<none>` repository/tag
display.

`loftd images sync <reference-or-selector>` preserves full image-reference sync
behavior and can also resolve a unique visible local selector, such as a
repository/tag prefix, digest prefix, or Buildah image-id prefix, before
materializing through Buildah. If no local visible row matches, the argument is
treated as the image reference to sync; ambiguous local selectors fail before
staging.

`loftd images remove <image-selector>` removes only a matching loftd cache entry.
It accepts exact full digests (`sha256:...`), exact digest keys
(`sha256-...`), and unique visible-row prefixes from `images list` such as
digest, repository/tag, selected reference, or Buildah image-id prefixes.
Ambiguous selectors are refused with candidate rows, and selectors that match
only `CACHE=uncached` Buildah rows are refused because there is no loftd cache
entry to delete. Removal remains cache-first: loftd attempts
`buildah rmi <selected-reference>` only when a fresh Buildah image inspect
proves the selected reference still resolves to the same digest recorded in the
cache metadata. Missing, digestless, ambiguous, or mismatched local Buildah
images are left in place and reported as skipped.

`loftd images remove --dry-run <image-selector>` resolves the same selector and
guard chain without mutating cache or local Buildah state. The preview reports
the exact loftd cache entry and the final local Buildah target that would be
removed after the existing fallback chain (selected reference, cached image ID,
then Buildah inventory reference). Unlike real remove, dry-run fails when that
local Buildah removal would be skipped for any reason.

Loftd uses **task rootfs backend** terminology for the host-side mechanism that
materializes the clean task root filesystem. The default backend is
`btrfs-snapshot`: loftd keeps a digest-keyed btrfs image-source snapshot cache
under its per-user image state directory and snapshots that cached source into a
fresh per-task rootfs on same-digest restarts. Cache misses still use one
`buildah unshare` transaction to create a temporary Buildah working container,
mount the selected image rootfs, validate exactly one executable
`loftd-guest-init`, snapshot the mounted rootfs into loftd task state, and
remove the Buildah working container; known-digest misses then snapshot that task
rootfs into the digest-keyed source cache and write cache metadata. Cache hits
may inspect/refresh image metadata but avoid the Buildah working-container
lifecycle (`buildah from`, `mount`, `umount`, and `rm`). Unknown-digest runs use
the direct Buildah materialization path and do not write cache entries. There is
no `auto` backend, no initial loftd `reflink` backend, and no copy/reflink
fallback for the default btrfs path; choose `fuse-overlay` explicitly when the
future portable overlay path is wanted.

On a successful btrfs-snapshot run, loftd then resolves the image's executable
`loftd-guest-init`, writes a private hex-encoded `launch.conf` under the task
state directory, and supervises a keep-id helper namespace around
`<loftd-exe> internal libkrun-network-enter <launch.conf>`. Buildah remains the
OCI image/rootfs materialization and cleanup tool, but it is no longer the
UID/GID namespace adapter for the libkrun helper. The helper wrapper requires
util-linux `unshare`, `newuidmap`, `newgidmap`, and usable `/etc/subuid` plus
`/etc/subgid` entries for the invoking user. It maps the invoking host UID and
GID to the same IDs inside the helper namespace, maps the lower and upper ID
ranges through subordinate IDs, then runs the helper as namespace root with
retained capabilities so prepared-root bind mounts can be grafted without
turning host-user-owned sources such as `/workspace` into `root:root` in the
guest view. During host-side network setup, loftd temporarily uses the keep-id
filesystem UID/GID for helper state writes, then restores namespace-root
filesystem identity in the VM worker before prepared-root grafting. Missing
mapping support is a hard launch error instead of a silent fallback to
root-owned bind mounts. This path does not rely on Podman, idmapped
mounts, host `chown`, `:U` ownership mutation, or relaxed guest-init ownership
repair. The internal helper is also a network manager: it creates one private
network namespace holder for the loftd session, starts `pasta` with Podman-like
`--map-guest-addr 169.254.1.2` and `--dns-forward 169.254.1.1`, then forks the
VM worker into that namespace. When `--passt` is enabled, the helper creates an
`AF_UNIX` socketpair and starts `passt` with `--fd <child-fd>` before the VM
worker enters the private network namespace; the worker inherits the other fd
and passes it to libkrun with `krun_add_net_unixstream()`. This follows crun's
passt wiring, keeps published ports bound in the helper's host-facing network
namespace, and avoids creating passt control sockets on host `/tmp`. Missing `pasta`, unsupported
unprivileged namespace setup, or early proxy exit is a hard launch error
instead of a silent broken-host-alias fallback. The Nix `loftd`,
`loftd-prebuilt`, and development
shell paths include `pkgs.passt` so both `pasta` and `passt` are on `PATH`;
non-Nix invocations must provide those tools themselves.

For loftd guest-side debugging, `--guest-init <host-binary>` validates the
host binary as an executable regular file, discovers the image's existing
`/nix/store/.../bin/loftd-guest-init`, and bind-mounts the host binary
read-only over that exact in-image target. Loftd still execs the discovered
`/nix/store/.../bin/loftd-guest-init` guest path and preserves the same
`LOFTD_*`, `KRUN_CONFIG`, arguments, and final guest command; it does not copy
or chmod the task-rootfs `/nix/store` file.

The default network mode remains libkrun TSI: loftd does not add a libkrun
network device by default, but the libkrun VMM starts from the pasta-backed
namespace so the guest's Podman-like host aliases can reach the host at
`169.254.1.2`. Passing `--passt` opts into libkrun virtio-net/passt mode. In
that mode the VM worker starts an additional `passt` unix-socket backend inside
the same namespace, sets guest env `LOFTD_USE_PASST=1`, and calls
`krun_add_net_unixstream()` before `krun_start_enter()`. Both modes always
materialize `/etc/hosts` with:

```text
169.254.1.2    host.containers.internal host.docker.internal
```

Use repeatable `-p, --publish SPEC` to expose guest services on host ports.
In the default TSI mode, loftd supports only simple TCP
`HOST_PORT:GUEST_PORT` mappings through a two-hop path: `pasta` listens in the
host-facing helper namespace and forwards `HOST_PORT` into the VM worker's
private network namespace, while libkrun `krun_set_port_map()` maps guest
listens on `GUEST_PORT` to that same target-namespace `HOST_PORT`:

```bash
./result/bin/loftd -p 8080:80 -- bash -lc 'python3 -m http.server 80'
```

TSI publish specs intentionally reject UDP, host bind addresses, port ranges,
random host ports, `all`/`none`, and protocol selectors. Use `--passt` when you
need broader passt-compatible forwarding syntax. In passt mode, unprefixed
publish specs default to TCP; `tcp:` and `udp:` select passt `-t` and `-u`
forwarding respectively, and passt owns deeper grammar validation for ranges,
bind-address suffixes, interfaces, and exclusions:

```bash
./result/bin/loftd --passt -p tcp:8080:80 -p udp:5353:5353 -- bash -lc 'echo ok'
```

Loftd still does not create a shared/global rootless network namespace.

Use repeatable `-v, --volume SOURCE:TARGET[:ro|:rw]` to add host bind mounts
to the prepared root. `SOURCE` may be a host file or directory; relative
sources are resolved from the workspace. `TARGET` must be an absolute guest
path. Omitting the mode defaults to read-write, `:rw` is explicit read-write,
and `:ro` remounts the bind target read-only after grafting:

```bash
./result/bin/loftd -v /host/cache:/home/dev/project-cache -- bash -lc 'ls /home/dev/project-cache'
./result/bin/loftd --volume /host/config.json:/workspace/config.json:ro -- cat /workspace/config.json
```

User volumes are additive only: they do not replace `/workspace`, `/nix`, or
the built-in Codex/Pi/Cargo/sccache/container-store mounts, and duplicate guest
targets are rejected. Loftd intentionally does not support Podman SELinux
suffixes (`:z`, `:Z`), ownership mutation (`:U`), propagation flags, named
volumes, or anonymous volumes.

After networking is ready, the helper dynamically loads `libkrun.so.1` or
`libkrun.so` from `$out/lib/loftd` when running from a Nix `.#loftd` package,
then falls back to normal soname lookup; `LOFTD_LIBKRUN_LIBRARY` still wins when
set. Host tool lookup follows the same wrapper-free pattern: per-tool overrides
(`LOFTD_BUILDAH`, `LOFTD_BTRFS`, `LOFTD_MKFS_BTRFS`, `LOFTD_BLKID`,
`LOFTD_PASTA`, `LOFTD_PASST`) win first, then `LOFTD_HELPER_BINARY_DIR`, then
`$out/libexec/loftd-helpers`, then `PATH` for source/debug runs. The helper
prepares a crun-style root export inside that same rootless namespace, and attaches that single prepared
root plus the writable persistent container-store disk. The prepared root is a
bind-mounted view of the task rootfs with the workspace, Codex, Pi, Cargo,
sccache, and host-prepared `/nix` overlay directories grafted into their final
guest paths before `krun_set_root`. Loftd
intentionally does not register one `krun_add_virtiofs3` device per developer
path; keeping those binds inside the root export avoids the legacy x86
IRQ/device exhaustion that can otherwise occur before libkrun's implicit vsock
device is registered.

On a host with libkrun and unprivileged namespace support, smoke-test the alias
contract by starting a host listener and connecting from both modes:

```bash
# terminal 1
python3 -m http.server 18080 --bind 0.0.0.0

# terminal 2
./result/bin/loftd -- bash -lc 'getent hosts host.containers.internal && curl -fsS http://host.containers.internal:18080/'
./result/bin/loftd --passt -- bash -lc 'getent hosts host.docker.internal && curl -fsS http://host.docker.internal:18080/'
```

For `/nix`, normal loftd launches now use a workspace-scoped host kernel
overlayfs rather than attaching `loftd-nix.raw`. The lowerdir is the selected
image cache rootfs under
`$STATE/loftd/microvm/images/btrfs-snapshots/<digest-key>/rootfs/nix`; the
upper, work, merged, and lease files live under the workspace slug state root at
`$STATE/loftd/<workspace-slug>/nix-overlay/`. Host-overlay launches run the
libkrun helper transaction through `buildah unshare`, so the VM worker mounts
and later unmounts the overlay in the same rootless Buildah namespace that can
see the selected image-cache lowerdir. The mount still happens immediately
before prepared-root grafting, and the merged view is bound to guest `/nix`.
Existing `loftd-nix.raw` files are not migrated or deleted automatically.
When a mutable image tag resolves to a new digest, the host-overlay lowerdir
follows the newly selected image cache entry while the workspace-scoped upper,
work, and merged directories are reused. This intentionally preserves packages
or files written into the overlay upperdir while exposing updated lower-image
store objects that are not shadowed by upperdir entries or overlay whiteouts.
It is not a Nix database merge or repair step: persistent Nix profiles, gcroots,
database rows, and whiteouts can still describe a mixed state and may require
manual cleanup or a workspace overlay reset if they become inconsistent.

- host-overlay `/nix` is signaled to the guest with `LOFTD_NIX_OVERLAY=1` and
  `LOFTD_NIX_HOST_OVERLAY=1`; no `/nix` disk id/label is emitted in this mode.
- host-overlay `/nix` requires `buildah` on `PATH`; permission-denied kernel
  overlay failures should be diagnosed from inside `buildah unshare`, because
  plain outer-namespace `mount -t overlay` does not have the required rootless
  idmap/storage context.
- Nested/rootless Podman storage uses the workspace-scoped
  `loftd-containers.raw` btrfs disk by default. The host exposes that disk as
  `LOFTD_CONTAINERS` / `loftd-containers` for guest rootless container storage,
  and guest Podman uses the `btrfs` storage driver.
- `loftd --container-store raw-disk` remains accepted as an explicit
  compatibility spelling for the only supported container-store backend.
  `--container-store bind` is not supported, and loftd does not migrate old
  host-directory container stores.

`loftd-guest-init enter` reads only `LOFTD_*` guest contract variables, validates
that the prepared-root paths already exist, ensures `/tmp` is a tmpfs with
`rw,exec,mode=1777`, verifies `/dev/net/tun` is the expected character device
`10:200`, makes it mode `0666`, probes it with `TUNSETIFF`, verifies the
host-prepared `/nix` overlay in host-overlay mode, prepares the selected
raw-disk container-store backend, exports the shell environment, and runs `fish -l` by
default. For deterministic smoke tests, `loftd -- <command>` preserves the same
guest bootstrap path but replaces the final guest command with the explicit argv
after `--`.

Loftd direct-libkrun mode requests nested virtualization before guest entry with
libkrun's `krun_check_nested_virt`/`krun_set_nested_virt` APIs, matching the
crun `krun.nested_virt=1` flow used by agentbox's OCI/libkrun path. This exposes
VMX/SVM to the guest when the host or outer VM already supports nested KVM; it
does not bind-mount host `/dev/kvm` and does not create `/dev/kvm` manually. The
node should appear from the guest KVM driver and devtmpfs, after which
`loftd-guest-init` makes it world-accessible for the default non-root task user.

If `/dev/kvm` is still absent inside the loftd guest, confirm the host has
`/dev/kvm`, then check the relevant host nested parameter: Intel hosts should
report `Y` or `1` from `/sys/module/kvm_intel/parameters/nested`, and AMD hosts
should report `Y` or `1` from `/sys/module/kvm_amd/parameters/nested`. Also
confirm the active libkrun firmware/kernel is KVM-capable (`CONFIG_KVM=y` plus
the relevant `CONFIG_KVM_INTEL=y` and/or `CONFIG_KVM_AMD=y`) and that devtmpfs is
enabled. Guest-side diagnostics usually start with
`dmesg | grep -Ei 'kvm|vmx|svm'`.

Phase 4 completion was validated with targeted `loftd` and `loftd-guest-init`
unit tests plus a focused local-image libkrun smoke test. The smoke used a local
`localhost/loftd:latest` image and verified Buildah-backed btrfs rootfs
materialization, persistent disk preparation, launch-config handoff, and a
successful libkrun guest-init entry. Full public-image publication and broader
guest-bootstrap hardening are follow-on work.

Loftd config lives at:

```text
$XDG_CONFIG_HOME/loftd/loftd.toml
```

or, when `XDG_CONFIG_HOME` is unset:

```text
$HOME/.config/loftd/loftd.toml
```

Supported launch-planning keys are:

```toml
[state]
location = "/home/dev/loftd-state"

[task-rootfs]
backend = "btrfs-snapshot" # or "fuse-overlay"
```

`[state].location` changes the base loftd state location; loftd appends
`/loftd/<workspace-slug>`. `--rootfs-backend` overrides
`[task-rootfs].backend` for a single run.

---

## Persistent host mounts

Each run ensures these host-backed paths and grafts them into the prepared root:

- current workspace -> `/workspace`
- `~/.codex` -> `/home/dev/.codex`
- `~/.pi` -> `/home/dev/.pi`
- `<state-root>/cargo` -> `/home/dev/.cargo`
- `<loftd-state>/sccache` -> `/home/dev/.cache/sccache`
- each `-v, --volume SOURCE:TARGET[:ro|:rw]` -> the requested absolute `TARGET`

This keeps Codex, Pi, Cargo, and compiler-cache state outside the repo while
matching the existing agentbox task-volume contract.

---

## State root and config

Default state root:

```text
$XDG_STATE_HOME/agentbox/<repo-slug>
```

Fallback when `XDG_STATE_HOME` is unset:

```text
$HOME/.local/state/agentbox/<repo-slug>
```

Override base location in:

```text
$XDG_CONFIG_HOME/agentbox/agentbox.toml
```

or:

```text
$HOME/.config/agentbox/agentbox.toml
```

Example:

```toml
[state]
location = "/home/dev/xxx/"
```

This makes the base `/home/dev/xxx/agentbox`.

Agentbox also keeps a shared sccache at:

```text
<state.location>/agentbox/sccache
```

That directory is bind-mounted into each task container at
`/home/dev/.cache/sccache`, so compiler cache entries are reused across
agentbox repos and containers.

---

## Container environment summary

The container provides:

- interactive `fish` + `starship`
- Codex CLI, bubblewrap (`bwrap`), Pi (`pi`), OMP (`omp`), and `oh-my-codex` (`omx`)
- cargo-deny and Symposium (`cargo-agents`, invoked as `cargo agents`)
- prebuilt OMX native helpers (`omx-api`, `omx-runtime`, and `omx-sparkshell`) with matching `OMX_*` binary override environment variables preset
- Python 3 (`PyYAML`, Tree-sitter, Tree-sitter Rust parser), Node.js
- Rust toolchain (`cargo`, `rustc`, `clippy`, `rustfmt`, `rust-analyzer`, `sccache`, `mold`)
- `gcc`, `musl`, `clang`
- `mimalloc` enabled by default for Nix-linked dynamic binaries through `/etc/ld-nix.so.preload`; `agentbox --hardened` and `loftd --hardened` switch task runs to GrapheneOS `hardened_malloc`, while `hardening-run` remains the per-command foreign/FHS `LD_PRELOAD` opt-in
- RTK (`rtk`)
- libkrun 1.18.0 (`libkrun.so`) plus pinned `libkrunfw.so` for nested KVM support inside the container
- `nix` wrapper that clears the container NSS wrapper preload before invoking
  the real Nix binary, avoiding glibc-version mismatches in nested dev shells
- `agentbox-nix-store-db-check` for non-mutating live `/nix/store` vs Nix DB
  validity diagnostics, including cautious libkrun upperdir store-layer
  evidence when `/run/agentbox/nix-disk/upper` is visible
- `rustc` and `rust-analyzer` wrappers that mask `/etc/ld-nix.so.preload` so
  both tools keep the default allocator
- `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER` preset to the bundled
  `clang_mold_wrapper` helper for the `x86_64-unknown-linux-gnu` target
- `LIBCLANG_PATH` preset to the bundled Nix `libclang` library directory
- `RUSTC_WRAPPER`, `CMAKE_C_COMPILER_LAUNCHER`, and `CMAKE_CXX_COMPILER_LAUNCHER` preset to the bundled `sccache`
- `SCCACHE_DIR=/home/dev/.cache/sccache`, backed by the shared host cache under the agentbox state root
- `/usr/bin/env` compatibility for common env-based shebangs such as
  `#!/usr/bin/env bash`
- narrow hardcoded-interpreter compatibility for `/bin/sh`, `/bin/bash`,
  `/bin/python`, and `/bin/python3`; `/bin/python` resolves to Python 3
  (not broad FHS compatibility)
- common tools (`curl`, `jq`, `openssl`, `tmux`, `rmux`, etc.); `tmux` comes
  from Nixpkgs in both the agentbox and loftd images, and the pinned `rmux`
  release remains available separately as `rmux`. `/etc/rmux.conf` is the
  image-level rmux config path. The default `rmux` config includes
  tmux-compatible bindings for horizontal/vertical splits (`|`, `-`) and pane
  selection with `h`, `j`, `k`, and `l`

`clang_mold_wrapper` keeps the default linker policy in the image and avoids
setting `RUSTFLAGS`, so existing Cargo config can still layer on top normally.
If `clang -fuse-ld=mold` ever stops resolving correctly in-image, the fallback
is to pin `mold` explicitly inside the wrapper and update this document to
match.

Container task launches use Podman `--userns=keep-id`; libkrun task launches
use loftd's keep-id helper namespace to provide the same `/workspace` ownership
contract for the guest dev user. The `--root` flag keeps the final shell as
root, but does not otherwise change the persistent host mount layout.

---

## Publishing

### Container image (GitHub Actions)

On push to `main`, push to `dev`, and tag pushes, CI publishes separate images:

- `ghcr.io/<repo-owner>/agentbox:latest` (main only)
- `ghcr.io/<repo-owner>/agentbox:dev` (dev only)
- `ghcr.io/<repo-owner>/agentbox:<git-tag>` (tag only)
- `ghcr.io/<repo-owner>/agentbox:sha-<12-char-commit>`
- `ghcr.io/<repo-owner>/loftd:latest` (main only)
- `ghcr.io/<repo-owner>/loftd:dev` (dev only)
- `ghcr.io/<repo-owner>/loftd:<git-tag>` (tag only)
- `ghcr.io/<repo-owner>/loftd:sha-<12-char-commit>`

The agentbox image is built from `.#agentbox-container` and verifies
`agentbox-guest-init`. The loftd image is built from `.#container` and verifies
`loftd-guest-init`. Agentbox image names are not aliases for loftd image names
while loftd remains incomplete.

### Prebuilt binaries (GitHub Releases)

Main-branch CI also publishes prerelease binary assets:

- rolling `alpha`
- commit-specific `sha-<12-char-commit>`

Older `sha-*` prereleases are pruned (retains newest 20).

The `agentbox-<arch>-unknown-linux-musl` asset is the portable static/musl
agentbox CLI. The `loftd-<arch>-unknown-linux-gnu` asset is a neutral dynamic
Linux ELF packaging input and intentionally non-standalone: it must not contain
release-builder `/nix/store/<hash>-...` references, and Nix packaging patches
its ordinary ELF runtime dependencies before wiring the libkrun/runtime-tool
environment.
For ordinary source-built loftd usage with pinned prebuilt libkrun firmware,
prefer `nix build .#loftd`; use `nix build .#loftd-prebuilt` only for the
explicit pinned release-asset packaging path with the same wrapper-free helper
layout, or the published
`ghcr.io/<repo-owner>/loftd` image. Use `nix build ./nix/dev#loftd-dev`
only from a local checkout with initialized `deps/libkrun` and `deps/libkrunfw`
submodules when local libkrun/libkrunfw experiments are intended; `github:`
downstream consumers should use root non-dev outputs.

---

## Maintenance helpers

Refresh pinned prebuilt release in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-agentbox-prebuilt.sh
```

Refresh pinned loftd prebuilt release metadata in `nix/pins.nix` from a neutral
raw-ELF `sha-*` release. The updater rejects wrapper-script assets, legacy
flake-locked names, and payloads containing concrete
`/nix/store/<hash>-...` references:

```bash
nix develop --command ./scripts/update-loftd-prebuilt.sh
```

Refresh pinned RTK prebuilt release metadata in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-rtk-prebuilt.sh
```

Refresh pinned Helvesec/rmux prebuilt release metadata in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-rmux-prebuilt.sh
```

Refresh pinned `zeroqn/libkrun` prebuilt release metadata in `nix/pins.nix`
from the newest matching `loftd-*` tag that contains both required Linux assets.
Root `.#libkrun` and every shared consumer (`.#crun`, `.#podman`, `.#agentbox`,
`.#loftd`, images, and `.#loftd-prebuilt`) use the same pinned prebuilt libkrun
package. Local source builds stay in the submodule-aware dev flake and use the
checked-out `deps/libkrun` submodule:

```bash
nix develop --command ./scripts/update-libkrun.sh
```

Refresh pinned `zeroqn/libkrunfw` release metadata in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-libkrunfw.sh
```

Refresh pinned Pi coding agent source/npm metadata in `nix/pins.nix` from `earendil-works/pi`:

```bash
nix develop --command ./scripts/update-pi-coding-agent.sh
```

Refresh pinned `omp` prebuilt release metadata in `nix/pins.nix` from `can1357/oh-my-pi`:

```bash
nix develop --command ./scripts/update-omp-prebuilt.sh
```

Refresh pinned `oh-my-codex` version/hashes in `nix/pins.nix` (including bundled Linux-musl native helper asset hashes):

```bash
nix develop --command ./scripts/update-oh-my-codex.sh
```

---

## Use from another flake (prebuilt binary)

```nix
{
  inputs.agentbox.url = "github:zeroqn/agentbox";

  outputs = { self, nixpkgs, agentbox, ... }: {
    nixosConfigurations.my-host = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ({ pkgs, ... }: {
          environment.systemPackages = [
            agentbox.packages.${pkgs.system}.agentbox-prebuilt
          ];
        })
      ];
    };
  };
}
```

For a source-build fallback, use:

```nix
agentbox.packages.${pkgs.system}.agentbox
```

Downstream flakes that install `.#loftd` or `.#loftd-prebuilt` receive the
loftd host-side default policy at:

```text
$out/share/loftd/seccomp/default.json
```

Downstream flakes can also depend on the separate packaged guest/container
seccomp policy via:

```nix
agentbox.packages.${pkgs.system}.container-lib-policy-seccomp-json
```
