# agentbox

`agentbox` is a small Rust CLI that starts an interactive Podman container shell
for your current project.

It mounts the current directory at `/workspace`, persists Codex/Cargo state on
the host, and runs Nix through a rootless sidecar stack by default.

Current runtime split:

- **Container mode (default):** native Podman task container plus host
  `fuse-overlayfs` and a reusable `nix-daemon` sidecar. This is the supported
  working mode.
- **Libkrun mode (explicit opt-in):** Podman + crun/libkrun VM mode with one
  sparse raw btrfs data image attached through `krun.disk.0.*` annotations.
  The guest uses that disk for a persistent kernel overlay at `/nix`, and the
  `/workspace` bind mount uses `--userns=keep-id` so ownership matches the host
  user after the guest drops privileges.

Seeded `/nix` copy fallback has been removed. Disabling the sidecar now fails
instead of copying `/nix/store` into agentbox state.

---

## Prerequisites

- Linux
- `podman`
- `nix` (for building via flake)
- `fuse-overlayfs` (required for default container sidecar mode; included by
  the `.#agentbox-prebuilt` package runtime environment)
- `mkfs.btrfs` and `blkid` on the host for first-time libkrun raw-image
  creation and reuse validation (`btrfs-progs` + `util-linux`; included in
  `nix develop`)
- libkrun mode additionally requires Podman using the custom crun/libkrun stack
  that supports `krun_add_disk` annotations plus guest kernel overlay and btrfs
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
  `fuse-overlayfs` into the runtime environment for default container sidecar
  mode.
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
nix develop --command cargo run -- --help
```

Build image + binary, then run the default container mode:

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

---

## Runtime modes

### 1) Container mode (default)

Run:

```bash
./result/bin/agentbox
```

`--native` is kept as a deprecated compatibility alias for the same container
mode. It is no longer required:

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
fail clearly:

```bash
./result/bin/agentbox --disable-nix-sidecar
AGENTBOX_NIX_SIDECAR=0 ./result/bin/agentbox
./result/bin/agentbox --sidecar-only --disable-nix-sidecar
AGENTBOX_NIX_SIDECAR=0 ./result/bin/agentbox --sidecar-only
```

#### Sidecar-only debugging

Start or reuse just the nix-daemon sidecar stack, print the sidecar name and
host proxy port, and exit without launching the interactive task container:

```bash
./result/bin/agentbox --sidecar-only
```

`--sidecar-only` intentionally leaves the sidecar container and merged nix
overlay running after exit so they can be inspected. It skips the nix-daemon
socket health probe so a broken daemon can still be debugged after container
startup.

Use the printed sidecar name for inspection and cleanup, for example:

```bash
podman logs <sidecar-name>
podman port <sidecar-name> 19876
podman rm -f <sidecar-name>
```

---

### 2) Libkrun mode (persistent raw-image `/nix` path)

Libkrun mode is an explicit opt-in VM path:

```bash
./result/bin/agentbox --libkrun
./result/bin/agentbox --libkrun --mem 8
./result/bin/agentbox --libkrun --tsi
```

On first run, agentbox creates a sparse btrfs raw image at:

```text
<state-root>/libkrun-nix.raw
```

The default apparent size is `64 GiB`. Because the file is sparse, host disk
usage grows as blocks are written, but the guest-visible capacity is still the
raw file's apparent size at VM start.

The raw image is attached with crun annotations:

```text
run.oci.handler=krun
krun.disk.0.path=<state-root>/libkrun-nix.raw
krun.disk.0.id=agentbox-nix
krun.disk.0.readonly=false
```

When `--tsi` is passed, agentbox also requests crun's libkrun passt network
path with `krun.use_passt=1`.

Inside the libkrun guest, the entrypoint finds the attached btrfs disk by label
(`AGENTBOX_NIX`), mounts it under `/run/agentbox/nix-disk`, bind-mounts the
image-provided `/nix` as a read-only lowerdir, and mounts a kernel overlay at
`/nix` using disk-backed upper/work directories. After the overlay is active, it
starts an in-guest `nix-daemon`, exports
`NIX_REMOTE=unix:///nix/var/nix/daemon-socket/socket`, verifies the socket before
privilege drop, then runs the shell as the host UID/GID. The libkrun task also
uses `--userns=keep-id`, so `/workspace` ownership matches the host user while
the image-default root entrypoint can still perform root-only `/nix` bootstrap.
The daemon is tied to the VM/container lifecycle and is not separately
supervised in v1.

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
./result/bin/agentbox --libkrun \
  --libkrun-debug-entrypoint /tmp/agentbox-libkrun-debug-entrypoint
```

The script is bind-mounted read-only at `/bin/agentbox-debug-entrypoint` and used
as the container entrypoint. The usual interactive shell (`fish -l`) is still
passed as arguments, so `exec "$@"` opens the shell after printing diagnostics.
This debug path runs as root and intentionally skips the normal `/nix` bootstrap
and host UID/GID privilege drop.

Existing raw images are reused only if `blkid` reports btrfs. Agentbox refuses
to overwrite invalid existing images.

Manual resize flow for v1:

```bash
# Stop any running libkrun VM first.
truncate -s 128G <state-root>/libkrun-nix.raw
./result/bin/agentbox --libkrun
```

On restart, the guest entrypoint attempts `btrfs filesystem resize max` on the
mounted data disk so the filesystem consumes the larger apparent image size. No
live auto-resize, state migration, multi-disk management, snapshot/rollback UX,
or root-disk mutation is implemented.

`--tsi` and `--mem` remain libkrun-only options and are rejected unless
`--libkrun` is also present. `--native --libkrun` is rejected as conflicting mode
selection.

Libkrun mode intentionally does **not** use the container sidecar/overlay bridge,
does **not** set `AGENTBOX_NIX_PROXY_HOST`, and does **not** fall back to seeded
Nix state.

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
- Codex CLI, OpenCode (`opencode`), Pi (`pi`), and `oh-my-codex` (`omx`)
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

Refresh pinned Pi coding agent metadata in `nix/pins.nix` from `badlogic/pi-mono`:

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
