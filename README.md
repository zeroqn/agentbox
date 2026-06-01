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
  the Nix `.#agentbox` package wraps the binary with this repo's
  libkrun/libkrunfw library path, and source/debug builds can set
  `AGENTBOX_LIBKRUN_LIBRARY=/path/to/libkrun.so.1` for agentbox microvm or
  `LOFTD_LIBKRUN_LIBRARY=/path/to/libkrun.so.1` for loftd.
- `btrfs`, `mkfs.btrfs`, and `blkid` on the host for microvm btrfs-snapshot
  storage, first-time libkrun/microvm raw-image creation, and reuse validation
  (`btrfs-progs` + `util-linux`; included in `nix develop`, and `btrfs` is
  included in the Nix `.#agentbox` and `.#agentbox-prebuilt` runtime wrappers).
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

The container also enables GrapheneOS `hardened_malloc` for Nix-linked dynamic
binaries through `/etc/ld-nix.so.preload`, matching NixOS' allocator preload
mechanism rather than setting a global allocator `LD_PRELOAD`. `rustc` and
`rust-analyzer` are started through wrappers that mask `/etc/ld-nix.so.preload`
for those processes so they keep the default allocator. Foreign/FHS glibc
binaries usually read `/etc/ld.so.preload` instead of `/etc/ld-nix.so.preload`,
while static or musl binaries generally ignore both files. For a specific
foreign/FHS command, opt in with:

```bash
hardening-run some-foreign-binary --flag
```

`hardening-run` sets `LD_PRELOAD` only for the wrapped command, so the usual
opt-out remains:

```bash
env -u LD_PRELOAD some-foreign-binary --flag
```

---

## Build

```bash
nix build .#loftd
nix build .#agentbox-prebuilt
nix build .#agentbox-musl
nix build .#rtk-prebuilt
nix build .#reasonix
nix build .#libkrunfw
nix build .#libkrun
nix build .#crun
nix build .#podman
nix build .#container-lib-policy-seccomp-json
nix build .#container
```

### Build outputs

- `.#agentbox`: compile from source.
- `.#loftd`: compile from source as the dynamic host `loftd` package. It uses
  the same Rust package output as `.#agentbox`, which wraps both host binaries
  with this flake's runtime library path.
- `.#agentbox-prebuilt`: install pinned published binary (currently pinned for
  `x86_64-linux`; use `.#agentbox` elsewhere). This package brings
  `fuse-overlayfs` and `buildah` into the runtime environment for
  `agentbox container` sidecar mode and experimental `agentbox microvm`
  cache misses.
- `.#agentbox-musl`: static/musl `agentbox`, `agentbox-guest-init`, and
  `loftd-guest-init` binaries for image/guest use. It intentionally does not
  build or expose `bin/loftd`; the host `loftd` binary is always dynamically
  linked so it can load `libkrun.so`/`libkrunfw.so` from the package or dev
  shell runtime library path.
- `.#reasonix`: build the pinned DeepSeek-Reasonix CLI from the release source rev.
- `.#rtk-prebuilt`: install the pinned published RTK release asset (currently
  pinned for `x86_64-linux`).
- `.#libkrunfw`: install the pinned `zeroqn/libkrunfw` release asset for the
  current system.
- `.#libkrun`: build libkrun 1.18.0 from source (overrides nixpkgs 1.17.4)
  with net, sound, GPU, block, and input support enabled.
- `.#crun`: build `zeroqn/crun` branch `agentbox` with this repo's libkrun
  override, krun handler support, raw data disk annotation support,
  `krun.nested_virt` support, and `pkgs.passt` on crun's runtime `PATH`.
- `.#podman`: build Podman against the custom crun for libkrun/raw-image
  development.
- `.#container-lib-policy-seccomp-json`: install the pinned
  `containers/container-libs` `common/pkg/seccomp/seccomp.json` policy at
  `share/containers/seccomp.json` for downstream flakes or image reuse.
- `.#container`: loftd-compatible Podman image archive named
  `localhost/loftd:latest`. This is the only container image build target; it
  packages both `loftd-guest-init` and `agentbox-guest-init`, so agentbox also
  uses it by default.

### Nix store / DB diagnostics

`nix build .#container` depends on a static image metadata linter before
running the layered image build command. To run only that linter:

```bash
nix build .#checks.$(nix eval --raw --impure --expr builtins.currentSystem).container-nix-db-metadata
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
nix build .#container
podman load < result
nix build .#loftd
./result/bin/loftd --help
```

Image selection behavior:

- default: `localhost/loftd:latest`
- fallback: `ghcr.io/zeroqn/loftd:latest`

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
`.#container-lib-policy-seccomp-json` and `.#container`.

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

Inside the shared loftd image, the configured image entrypoint is
`loftd-guest-init enter --`. Agentbox does not depend on that image
entrypoint: it explicitly starts `/bin/agentbox-guest-init default enter --`
from the same image. That default agentbox guest entrypoint selects the
explicit `libkrun` guest runtime when agentbox passes `AGENTBOX_LIBKRUN_*`
environment flags; otherwise it falls back to the explicit `container` guest
runtime. The explicit `agentbox-guest-init container enter` path does not
switch to libkrun.

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

Current limitations: networking relies on libkrun's no-passt/TSI default path, direct
microvm inbound port publishing is intentionally unavailable, terminal resize has only the
narrow default virtio-console hook wired so far, and real VM smoke validation is
still pending. See `docs/microvm-smoke.md` for the manual smoke checklist and
current not-tested items.

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
`/nix` and rootless container storage, starts a same-binary
`loftd internal libkrun-enter` helper to call libkrun, and enters the guest
through `loftd-guest-init enter`. The parent process owns task-rootfs cleanup,
with best-effort cleanup on unwind and `--preserve-debug` for manual inspection.
The explicit `fuse-overlay` backend is still a future slice.

Run/help:

```bash
./result/bin/loftd --help
./result/bin/loftd --rootfs-backend btrfs-snapshot
./result/bin/loftd --rootfs-backend fuse-overlay
./result/bin/loftd --pull-latest
./result/bin/loftd --image ghcr.io/example/loftd:dev
./result/bin/loftd -- bash -lc 'echo ok'
```

Image selection is materialized through Buildah for the btrfs-snapshot path: with no
image option, loftd first inspects `localhost/loftd:latest` and uses it with
`--pull=never` when present, otherwise loftd uses `ghcr.io/zeroqn/loftd:latest`
with `--pull=missing`. The flake's canonical `.#container` output builds that
local `localhost/loftd:latest` image with `loftd-guest-init enter` as its guest
contract. `--pull-latest` uses the canonical image with `--pull=always`, and
`--image` uses exactly the supplied image reference with `--pull=missing`.
`--image` and `--pull-latest` are mutually exclusive.

Loftd uses **task rootfs backend** terminology for the host-side mechanism that
materializes the clean task root filesystem. The default backend is
`btrfs-snapshot`: one `buildah unshare` transaction creates a temporary Buildah
working container, mounts the selected image rootfs, validates exactly one
executable `loftd-guest-init`, snapshots the mounted rootfs into loftd task
state, and removes the Buildah working container. There is no `auto` backend,
no initial loftd `reflink` backend, and no copy/reflink fallback for the default
btrfs path; choose `fuse-overlay` explicitly when the future portable overlay
path is wanted.

On a successful btrfs-snapshot run, loftd then resolves the image's executable
`loftd-guest-init`, writes a private hex-encoded `launch.conf` under the task
state directory, and supervises `loftd internal libkrun-enter <launch.conf>`.
The helper dynamically loads `libkrun.so.1` or `libkrun.so` (or
`LOFTD_LIBKRUN_LIBRARY` when set), attaches the task rootfs, the `/workspace`
virtiofs mount, and two writable persistent disks:

- `loftd-nix.raw` exposed to the guest as `LOFTD_NIX` / `loftd-nix` for `/nix`;
- `loftd-containers.raw` exposed as `LOFTD_CONTAINERS` / `loftd-containers` for
  rootless container storage.

`loftd-guest-init enter` reads only `LOFTD_*` guest contract variables, mounts
the workspace before identity drop, prepares the persistent cache disks, exports
the shell environment, and runs `fish -l` by default. For deterministic smoke
tests, `loftd -- <command>` preserves the same guest bootstrap path but replaces
the final guest command with the explicit argv after `--`.

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

Each run ensures and mounts:

- `~/.codex` -> `/home/dev/.codex`
- `~/.pi` -> `/home/dev/.pi`
- `<state-root>/cargo` -> `/home/dev/.cargo`

This keeps Codex, Pi, and Cargo state outside the repo.

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
- Codex CLI, bubblewrap (`bwrap`), Pi (`pi`), Reasonix (`reasonix`/`dsnix`), and `oh-my-codex` (`omx`)
- cargo-deny and Symposium (`cargo-agents`, invoked as `cargo agents`)
- prebuilt OMX native helpers (`omx-api`, `omx-runtime`, `omx-sparkshell`, and `omx-explore-harness`) with matching `OMX_*` binary override environment variables preset
- Python 3 (`PyYAML`, Tree-sitter, Tree-sitter Rust parser), Node.js
- Rust toolchain (`cargo`, `rustc`, `clippy`, `rustfmt`, `rust-analyzer`, `sccache`, `mold`)
- `gcc`, `musl`, `clang`
- GrapheneOS `hardened_malloc` enabled for Nix-linked dynamic binaries through `/etc/ld-nix.so.preload`, plus `hardening-run` for per-command foreign/FHS `LD_PRELOAD` opt-in
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
- common tools (`curl`, `jq`, `tmux`, etc.); tmux disables mouse support and
  includes system-wide pane split bindings for `Ctrl-b |` and `Ctrl-b -` plus
  Vim-style pane focus movement on `Ctrl-b h/j/k/l`

`clang_mold_wrapper` keeps the default linker policy in the image and avoids
setting `RUSTFLAGS`, so existing Cargo config can still layer on top normally.
If `clang -fuse-ld=mold` ever stops resolving correctly in-image, the fallback
is to pin `mold` explicitly inside the wrapper and update this document to
match.

Both container and libkrun task containers run with `--userns=keep-id` so
`/workspace` ownership matches host mapping. The `--root` flag keeps the final
shell as root, but does not otherwise change the persistent host mount layout.

---

## Publishing

### Container image (GitHub Actions)

On push to `main`, push to `dev`, and tag pushes, CI publishes to:

- `ghcr.io/<repo-owner>/loftd:latest` (main only)
- `ghcr.io/<repo-owner>/loftd:dev` (dev only)
- `ghcr.io/<repo-owner>/loftd:<git-tag>` (tag only)
- `ghcr.io/<repo-owner>/loftd:sha-<12-char-commit>`
- `ghcr.io/<repo-owner>/agentbox:*` with the same tags as a compatibility
  alias for existing agentbox users.

Both image names point at the same shared loftd-compatible image built from
`.#container`; CI does not build a separate agentbox image. The shared image
contains both `loftd-guest-init` and `agentbox-guest-init`, so loftd uses the
canonical image entrypoint while agentbox explicitly enters through its
compatibility guest init path.

### Prebuilt binaries (GitHub Releases)

Main-branch CI also publishes prerelease binary assets:

- rolling `alpha`
- commit-specific `sha-<12-char-commit>`

Older `sha-*` prereleases are pruned (retains newest 20).

The `agentbox-<arch>-unknown-linux-musl` asset is the portable static/musl
agentbox CLI. The `loftd-<arch>-linux-flake-locked` asset is dynamically linked
and intentionally non-standalone: it can rely on the exact Nix store closure
from this flake lock. For ordinary loftd usage, prefer `nix build .#loftd` or
the published `ghcr.io/<repo-owner>/loftd` image.

---

## Maintenance helpers

Refresh pinned prebuilt release in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-agentbox-prebuilt.sh
```

Refresh pinned RTK prebuilt release metadata in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-rtk-prebuilt.sh
```

Refresh pinned `zeroqn/libkrunfw` release metadata in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-libkrunfw.sh
```

Refresh pinned OpenCode release metadata in `nix/pins.nix` from `anomalyco/opencode`:

```bash
nix develop --command ./scripts/update-opencode.sh
```

Refresh pinned Pi coding agent source/npm metadata in `nix/pins.nix` from `earendil-works/pi`:

```bash
nix develop --command ./scripts/update-pi-coding-agent.sh
```

Refresh pinned Reasonix source/npm metadata in `nix/pins.nix` from the latest `esengine/DeepSeek-Reasonix` release target rev:

```bash
nix develop --command ./scripts/update-reasonix.sh
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

Downstream flakes can also depend on the packaged seccomp policy via:

```nix
agentbox.packages.${pkgs.system}.container-lib-policy-seccomp-json
```
