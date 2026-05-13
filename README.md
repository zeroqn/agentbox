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
  rootless Podman storage as `dev` with the Podman `btrfs` storage driver.
  The `/workspace` bind mount uses `--userns=keep-id` so ownership matches the
  host user after the guest drops privileges.
- **Container mode (`--native`):** native Podman task container plus host
  `fuse-overlayfs` and a reusable `nix-daemon` sidecar. `--sidecar-only` also
  selects this mode for sidecar debugging.

Seeded `/nix` copy fallback has been removed. Disabling the sidecar now fails
instead of copying `/nix/store` into agentbox state.

---

## Prerequisites

- Linux
- `podman`
- `nix` (for building via flake)
- `fuse-overlayfs` (required for `--native` container sidecar mode; included
  by the `.#agentbox-prebuilt` package runtime environment)
- `mkfs.btrfs` and `blkid` on the host for first-time libkrun raw-image
  creation and reuse validation (`btrfs-progs` + `util-linux`; included in
  `nix develop`)
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
  `fuse-overlayfs` into the runtime environment for `--native` container
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

Enable Podman debug logging for troubleshooting agentbox-managed Podman
commands:

```bash
./result/bin/agentbox --debug
./result/bin/agentbox --sidecar-only --debug
```

`--debug` passes `--log-level=debug` to Podman commands that agentbox runs,
including task launch, sidecar setup, image inspection/mounting, health probes,
and cleanup paths. It only changes Podman logging verbosity.

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
./result/bin/agentbox --libkrun
./result/bin/agentbox --mem 8
./result/bin/agentbox --tsi
```

`--libkrun` remains accepted as an explicit selector for the default runtime.
Libkrun-only options such as `--mem`, `--tsi`, and
`--libkrun-debug-entrypoint` are valid without adding `--libkrun`.

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
whole GiB, and emits that value with `krun.ram_mib=<MiB>`. Pass `--mem <GiB>` to
override it. On Linux, agentbox also emits `krun.cpus=<n>`: hosts with up to 6
available CPUs pass all CPUs through, while larger hosts reserve 2 CPUs for the
host. On non-Linux hosts or when CPU count is unavailable, `krun.cpus` is
omitted.

Libkrun mode enables passt networking by default with `krun.use_passt=1`. In
that default passt path, the normal image entrypoint also ensures the guest
resolver starts with `nameserver 169.254.1.1`, matching passt's DNS forwarder,
while preserving any existing resolver lines after it. When `--tsi` is passed,
agentbox switches to the TSI/proxy environment path instead: it omits
`krun.use_passt=1`, skips the passt resolver injection, and passes `no_proxy=1`
into the guest.

Inside the libkrun guest, the generated image entrypoint now acts as a small
trampoline for libkrun runs and immediately execs the Rust guest initializer:

```bash
agentbox-guest-init libkrun enter -- fish -l
```

Normal `--native` container mode intentionally stays on the existing Bash
entrypoint path. Set `AGENTBOX_GUEST_INIT_DISABLE=1` only for debugging the old
libkrun Bash fallback in the image.

`agentbox-guest-init` performs the root-required shell prerequisites before the
privilege drop: it writes real `/etc/passwd` and `/etc/group` entries for the
dynamic host UID/GID, creates/chowns `/home/dev` and the XDG home directories,
normalizes passt DNS when `krun.use_passt=1`, mounts the persistent `/nix`
overlay, starts `nix-daemon`, and waits for
`/nix/var/nix/daemon-socket/socket`. Those `/nix` steps remain blocking because
Nix-backed tools must not race shell startup.

Rootless Podman setup is lazy. Before dropping to `dev`, the Rust initializer
spawns root-required Podman prep in the background and records progress in:

```text
/run/agentbox/podman-prep.status
/run/agentbox/podman-prep.log
```

The background prep owns `/etc/subuid`, `/etc/subgid`, setuid
`newuidmap`/`newgidmap` helpers in `/run/agentbox/idmap-bin`, rootless user
namespace sysctls, `/dev/net/tun` permissions, the container btrfs disk mount at
`/home/dev/.local/share/containers`, and the rootless Podman config files:

```text
/home/dev/.config/containers/storage.conf
/home/dev/.config/containers/containers.conf
/home/dev/.config/containers/registries.conf
/home/dev/.config/containers/policy.json
```

The image `podman` command is a compatibility wrapper. In libkrun mode it first
runs `agentbox-guest-init libkrun podman wait`, which waits for the background
prep to become `ready` or reports a failed/stale/timeout status with the log
path, then execs the packaged Podman binary. Non-libkrun Podman wrapper behavior
is unchanged.

`storage.conf` is generated with `driver = "btrfs"`, graphroot
`/home/dev/.local/share/containers/storage`, and runroot
`/run/user/<dev-uid>/containers`. It intentionally has no `mount_program`, no
`overlay` driver fallback, and no `vfs` driver fallback. `containers.conf`
pins crun, conmon, cgroupfs, file events, and netavark/pasta helper paths for
this non-systemd guest, while setting `cgroups = "disabled"` so rootless nested
containers do not require systemd cgroup delegation or write access under
`/sys/fs/cgroup`. This means v1 nested guest containers do not provide
cgroup-based resource-limit enforcement. `registries.conf` leaves blocked and
insecure registry lists empty and sets the unqualified image search registry to
`docker.io` so commands such as `podman pull alpine` work inside the guest.
`policy.json` sets the default and `docker-daemon` transports to
`insecureAcceptAnything` so the guest has a local image signature policy for
development pulls.

The background prep also enables the guest kernel's rootless user namespace
knobs required by Podman: it raises `/proc/sys/user/max_user_namespaces` to at
least `28633`, and sets `/proc/sys/kernel/unprivileged_userns_clone=1` when that
distro-specific sysctl exists. First `podman` use fails clearly if the kernel
does not expose user namespace support or refuses those writes.

The Rust guest initializer finds the `/nix` btrfs disk by label (`AGENTBOX_NIX`),
mounts it under `/run/agentbox/nix-disk`, bind-mounts the image-provided `/nix`
as a read-only lowerdir, and mounts a kernel overlay at `/nix` using disk-backed
upper/work directories. During upperdir bootstrap, agentbox makes the overlaid
`/nix/store` directory owned by the `nixbld` group; the store directory mode is
`1775`, while store entries inherited from the image may remain `root:root`.
After the overlay is active, it starts an in-guest `nix-daemon`, exports
`NIX_REMOTE=unix:///nix/var/nix/daemon-socket/socket`, verifies the socket before
privilege drop, starts lazy Podman prep, then runs the shell as the host
UID/GID. The libkrun task also uses
`--userns=keep-id` plus `--user=0:0`, so `/workspace` ownership matches the host
user while the entrypoint still starts as root for `/run/agentbox` creation and
root-only bootstrap. The libkrun task passes the host `/dev/net/tun` through to
the guest at the same path, and the entrypoint makes the guest device node
world-readable/writable before dropping privileges, so nested rootless Podman can
bring up container networking. The daemon is tied to the VM/container lifecycle
and is not separately supervised in v1.

For guest-side debugging, pass a temporary entrypoint script to bypass the normal
image entrypoint and run custom diagnostics before handing off to the requested
command:

```bash
cat > /tmp/agentbox-libkrun-debug-entrypoint <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

echo "== agentbox libkrun debug entrypoint =="
id
printf 'working directory: %s\n' "$PWD"
printf 'command:'
printf ' %q' "$@"
printf '\n'

# Add temporary diagnostics here, for example:
# mount
# ls -la /workspace /nix /run/agentbox || true

exec "$@"
EOF
chmod +x /tmp/agentbox-libkrun-debug-entrypoint
./result/bin/agentbox --libkrun-debug-entrypoint /tmp/agentbox-libkrun-debug-entrypoint
```

The script is bind-mounted read-only at `/bin/agentbox-debug-entrypoint` and used
as the container entrypoint. The usual interactive shell (`fish -l`) is still
passed as arguments, so `exec "$@"` opens the shell after printing diagnostics.
This debug path runs as root and intentionally skips the normal `/nix` bootstrap
and host UID/GID privilege drop. It also skips normal entrypoint conveniences
such as the passt `/etc/resolv.conf` check, so add those diagnostics or setup
steps manually if the debug script needs them.

To test a modified `agentbox-guest-init` without rebuilding the container image,
build only the static guest-init binary and bind-mount it over the in-image
guest-init path:

```bash
nix build .#agentbox-musl -o result-musl
./result/bin/agentbox --libkrun-debug-guest-init ./result-musl/bin/agentbox-guest-init
```

This keeps the normal image entrypoint and shell arguments intact, but the
entrypoint executes the host-provided `agentbox-guest-init` binary. The host
`agentbox` binary must know the image guest-init path; the Nix-built
`./result/bin/agentbox` wires this automatically. If you are running a
non-Nix-built `agentbox` binary, set `AGENTBOX_LIBKRUN_GUEST_INIT_TARGET` to the
image path printed by the Nix build before using `--libkrun-debug-guest-init`.

Existing raw images are reused only if `blkid` reports btrfs. Agentbox refuses
to overwrite invalid existing images.

Manual resize flow for v1:

```bash
# Stop any running libkrun VM first.
truncate -s 128G <state-root>/libkrun-nix.raw
truncate -s 128G <state-root>/libkrun-containers.raw
./result/bin/agentbox
```

Resize only the disk that needs more space. On restart, the guest entrypoint
attempts `btrfs filesystem resize max` on each mounted data disk so the
filesystem consumes the larger apparent image size. No live auto-resize, state
migration/reset UX, snapshot/rollback UX, host-port helper UX, rootful nested
Podman workflow, or native-mode nested-Podman support is implemented.

Manual host smoke checklist for the nested rootless Podman feature:

1. Build and load `.#container`, then start default libkrun mode on the host.
2. Inside the guest, confirm the shell is `dev` and run `podman info`; verify
   rootless mode and storage driver `btrfs`.
3. Confirm `/dev/net/tun` exists inside the guest, then run
   `podman run --rm docker.io/library/alpine:latest echo hello` or an equivalent
   dev/test container.
4. Exit and restart agentbox; verify the pulled image/storage persists via
   `<state-root>/libkrun-containers.raw`.
5. Confirm no fuse-overlayfs path/config/binary is required by the rootless
   Podman setup.

`--native --libkrun` is rejected as conflicting mode selection. Container
sidecar controls such as `--sidecar-only`, `--disable-nix-sidecar`, and
`AGENTBOX_NIX_SIDECAR=0` are rejected with `--libkrun` because libkrun does not
use the container sidecar/overlay bridge.

Libkrun mode intentionally does **not** use the container sidecar/overlay bridge,
does **not** set `AGENTBOX_NIX_PROXY_HOST`, does **not** fall back to seeded Nix
state, and does **not** use fuse-overlayfs for nested rootless Podman storage.

---

### 2) Container mode (`--native`)

Run:

```bash
./result/bin/agentbox --native
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

#### Sidecar requirement

Container mode requires the sidecar. Seeded fallback has been removed, so these
fail clearly through container validation:

```bash
./result/bin/agentbox --native --disable-nix-sidecar
AGENTBOX_NIX_SIDECAR=0 ./result/bin/agentbox --native
./result/bin/agentbox --sidecar-only --disable-nix-sidecar
AGENTBOX_NIX_SIDECAR=0 ./result/bin/agentbox --sidecar-only
```

Without `--native` or `--sidecar-only`, `--disable-nix-sidecar` and
`AGENTBOX_NIX_SIDECAR=0` are rejected before launch because libkrun is the
default and does not use the container sidecar. Pass `--native` if you intended
container mode.

#### Sidecar-only debugging

Start or reuse just the container nix-daemon sidecar stack, print the sidecar
name and host proxy port, and exit without launching the interactive task
container:

```bash
./result/bin/agentbox --sidecar-only
```

`--sidecar-only` implicitly selects container mode. It intentionally leaves the
sidecar container and merged nix overlay running after exit so they can be
inspected. It skips the nix-daemon socket health probe so a broken daemon can
still be debugged after container startup. `--libkrun --sidecar-only` is rejected
as conflicting mode selection.

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
