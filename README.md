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
  rootless Podman plus rootless Docker storage as `dev` with `btrfs` storage
  drivers.
  The `/workspace` bind mount uses `--userns=keep-id` so ownership matches the
  host user after the guest drops privileges.
- **Container mode (`agentbox container`):** native Podman task container plus
  host `fuse-overlayfs` and a reusable `nix-daemon` sidecar.
  `agentbox container sidecar` starts or reuses only the sidecar stack for
  debugging.

Seeded `/nix` copy fallback has been removed. Container mode always uses the
managed sidecar.

---

## Prerequisites

- Linux
- `podman`
- `nix` (for building via flake)
- `fuse-overlayfs` (required for `agentbox container` sidecar mode; included
  by the `.#agentbox-prebuilt` package runtime environment)
- `mkfs.btrfs` and `blkid` on the host for first-time libkrun raw-image
  creation and reuse validation (`btrfs-progs` + `util-linux`; included in
  `nix develop`)
- `/dev/net/tun` on the host for libkrun mode, passed through to the guest so
  nested rootless Podman/Docker can set up TUN-backed networking.
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
Nix binary. This prevents nested dev shells from mixing the container preload
with a different glibc from the shell's realized dependencies. If you are using
an older image without that wrapper, use this temporary workaround:

```bash
env -u LD_PRELOAD -u NSS_WRAPPER_PASSWD -u NSS_WRAPPER_GROUP nix develop
```

---

## Build

```bash
nix build .#agentbox
nix build .#agentbox-prebuilt
nix build .#agentbox-musl
nix build .#rtk-prebuilt
nix build .#libkrunfw
nix build .#libkrun
nix build .#crun
nix build .#podman
nix build .#container
```

### Build outputs

- `.#agentbox`: compile from source.
- `.#agentbox-prebuilt`: install pinned published binary (currently pinned for
  `x86_64-linux`; use `.#agentbox` elsewhere). This package brings
  `fuse-overlayfs` into the runtime environment for `agentbox container`
  sidecar mode.
- `.#agentbox-musl`: static host binary.
- `.#rtk-prebuilt`: install the pinned published RTK release asset (currently
  pinned for `x86_64-linux`).
- `.#libkrunfw`: install the pinned `zeroqn/libkrunfw` release asset for the
  current system.
- `.#libkrun`: build libkrun 1.18.0 from source (overrides nixpkgs 1.17.4)
  with net, sound, GPU, block, and input support enabled.
- `.#crun`: build `zeroqn/crun` branch `agentbox` with this repo's libkrun
  override, krun handler support, raw data disk annotation support, and `pkgs.passt`
  on crun's runtime `PATH`.
- `.#podman`: build Podman against the custom crun for libkrun/raw-image
  development.
- `.#container`: Podman image archive.

---

## Quick start

Show CLI help:

```bash
nix develop --command cargo run -p agentbox-host -- --help
```

Build image + binary, then run the default libkrun mode:

```bash
nix build .#container
podman load < result
nix build .#agentbox
./result/bin/agentbox
```

Image selection behavior:

- default: `localhost/agentbox:latest`
- fallback: `ghcr.io/zeroqn/agentbox:latest`

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

Collect `agentbox-guest-init` component timings for the task container:

```bash
./result/bin/agentbox --profile --debug
./result/bin/agentbox container --profile --debug
```

`--profile` enables guest-init timing collection. Timings are printed only when
`--debug` is also set, and the report is written to stderr so stdout remains
reserved for command output. `--profile` without `--debug` enables measurement
but suppresses the report; `--debug` without `--profile` does not print a timing
report. Libkrun background Podman prep/wait workers and sidecar debug runs do
not emit guest-init profile reports. When libkrun `/nix` overlay bootstrap runs,
nested
`bootstrap-nix:*` rows break down disk discovery, mount/preseed work, daemon
startup, and the `bootstrap-nix:wait-socket` polling loop.

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
krun.disk.0.path=<state-root>/libkrun-nix.raw
krun.disk.0.id=agentbox-nix
krun.disk.0.readonly=false
krun.disk.1.path=<state-root>/libkrun-containers.raw
krun.disk.1.id=agentbox-containers
krun.disk.1.readonly=false
krun.use_passt=1
--device /dev/net/tun:/dev/net/tun
```

By default, agentbox sizes libkrun memory to 80% of host memory, rounded down to
whole GiB, and emits that value with `krun.ram_mib=<MiB>`. Pass
`agentbox libkrun --mem <GiB>` to override it. On Linux, agentbox also emits
`krun.cpus=<n>`: hosts with up to 6 CPUs pass all available CPUs through;
larger hosts reserve 2 CPUs for the host.

By default, libkrun mode uses passt networking through `krun.use_passt=1`. Pass
`agentbox libkrun --tsi` to switch to the older TSI/proxy environment path.

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

Restart-time btrfs auto-grow is not performed. Extending
`libkrun-nix.raw` or `libkrun-containers.raw` with `truncate` changes the
apparent raw device size, but agentbox no longer runs
`btrfs filesystem resize max` during guest initialization. A future explicit
`agentbox` resize command is expected to own that workflow; it is not
implemented yet.

No live auto-resize, state migration/reset UX, snapshot/rollback UX, host-port
helper UX, rootful nested Podman workflow, or container-mode nested-Podman support
is implemented.

Manual host smoke checklist for nested rootless container runtimes:

1. Build and load `.#container`, then start default libkrun mode on the host.
2. Inside the guest, confirm the shell is `dev` and run `podman info`; verify
   rootless mode and storage driver `btrfs`.
3. Run `docker info`; verify `Storage Driver: btrfs`, Docker root dir
   `/home/dev/.local/share/containers/docker/data`, rootless security options,
   and `Cgroup Driver: none`. Docker starts without systemd through the
   agentbox wrapper and `dockerd-rootless.sh`.
4. Confirm `/dev/net/tun` exists inside the guest, then run both:

   ```bash
   podman run --rm docker.io/library/alpine:latest echo hello
   docker run --rm docker.io/library/alpine:latest echo hello
   ```

5. Exit and restart agentbox; verify pulled Podman and Docker image/storage
   persists via `<state-root>/libkrun-containers.raw`. Docker persistent state
   should live under `/home/dev/.local/share/containers/docker`; `/var/lib/docker`,
   `/var/lib/containerd`, and `/home/dev/.local/share/docker` should be absent,
   empty, or symlink/bind-mounted into that Docker subtree.
6. For Docker troubleshooting, inspect `/run/agentbox/docker-prep.status`,
   `/run/agentbox/docker-prep.log`, `/run/user/$(id -u)/docker/daemon.status`, and
   `/run/user/$(id -u)/docker/daemon.log`.
7. Confirm no fuse-overlayfs path/config/binary is required by either rootless
   runtime setup.

Rootless Docker runs in this libkrun guest without a systemd user service or
delegated cgroup v2 controller. The expected Docker cgroup driver is therefore
`none`; Docker resource-limit flags that require cgroup delegation are not
supported in this environment.

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

## Persistent host mounts

Each run ensures and mounts:

- `~/.codex` -> `/home/dev/.codex`
- `<state-root>/cargo` -> `/home/dev/.cargo`

This keeps Codex + Cargo state outside the repo.

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
- Codex CLI, bubblewrap (`bwrap`), OpenCode (`opencode`), Pi (`pi`), and `oh-my-codex` (`omx`)
- prebuilt `omx-explore-harness` with `OMX_EXPLORE_BIN` preset to the bundled binary
- Python 3 (`PyYAML`, Tree-sitter, Tree-sitter Rust parser), Node.js
- Rust toolchain (`cargo`, `rustc`, `clippy`, `rustfmt`, `rust-analyzer`, `sccache`, `mold`)
- `gcc`, `musl`, `clang`
- RTK (`rtk`)
- libkrun 1.18.0 (`libkrun.so`) plus pinned `libkrunfw.so` for nested KVM support inside the container
- `nix` wrapper that clears the container NSS wrapper preload before invoking
  the real Nix binary, avoiding glibc-version mismatches in nested dev shells
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
- common tools (`curl`, `jq`, `tmux`, etc.)

`clang_mold_wrapper` keeps the default linker policy in the image and avoids
setting `RUSTFLAGS`, so existing Cargo config can still layer on top normally.
If `clang -fuse-ld=mold` ever stops resolving correctly in-image, the fallback
is to pin `mold` explicitly inside the wrapper and update this document to
match.

Both container and libkrun task containers run with `--userns=keep-id` so
`/workspace` ownership matches host mapping.

---

## Publishing

### Container image (GitHub Actions)

On push to `main`, push to `dev`, and tag pushes, CI publishes to:

- `ghcr.io/<repo-owner>/agentbox:latest` (main only)
- `ghcr.io/<repo-owner>/agentbox:dev` (dev only)
- `ghcr.io/<repo-owner>/agentbox:<git-tag>` (tag only)
- `ghcr.io/<repo-owner>/agentbox:sha-<12-char-commit>`

The published image keeps the musl `agentbox` binary in its own top image layer
so GHCR can reuse lower blobs when only the CLI binary changes.

### Prebuilt binaries (GitHub Releases)

Main-branch CI also publishes musl binaries as prereleases:

- rolling `alpha`
- commit-specific `sha-<12-char-commit>`

Older `sha-*` prereleases are pruned (retains newest 20).

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

Refresh pinned `oh-my-codex` version/hashes in `nix/pins.nix` (including the bundled `omx-explore-harness` asset hash):

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
