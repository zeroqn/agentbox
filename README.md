# agentbox

`agentbox` is a small Rust CLI that starts an interactive Podman container shell
for your current project.

It mounts the current directory at `/workspace`, persists Codex state from the
host, and supports two Nix runtime modes:

- **Rootless sidecar mode (default):** uses host `fuse-overlayfs` + a reusable
  `nix-daemon` sidecar (no `/nix/store` seed copy).
- **Seeded mode (fallback):** copies `/nix` into project state on first run.

By default, the interactive task container runs through crun's libkrun handler
while the Nix sidecar stays native. Use `--task-native` when you need the
interactive task container to run as a normal native Podman container.

---

## Prerequisites

- Linux
- `podman`
- `nix` (for building via flake)
- `fuse-overlayfs` (required for default sidecar mode; included by the
  `.#agentbox-prebuilt` package runtime environment)
- For the default libkrun task runtime: a host Podman/crun stack that supports
  `--runtime crun`, `run.oci.handler=krun`, `krun.use_passt=1`, and `/dev/kvm`.
  This flake provides `.#crun` with `passt` on crun's runtime `PATH`, and
  `.#podman` wired to that custom crun; install or otherwise expose `.#podman`
  as the host `podman` on `PATH` if you want `agentbox` to use it.

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
  `fuse-overlayfs` into the runtime environment for default sidecar mode.
- `.#agentbox-musl`: static host binary.
- `.#rtk-prebuilt`: install the pinned published RTK release asset (currently
  pinned for `x86_64-linux`).
- `.#libkrunfw`: install the pinned `zeroqn/libkrunfw` release asset for the
  current system.
- `.#libkrun`: build libkrun 1.18.0 from source (overrides nixpkgs 1.17.4)
  with net, sound, GPU, block, and input support enabled. Provides the
  repo-pinned libkrun used by the custom crun output and links it against the
  repo-pinned `libkrunfw.so`.
- `.#crun`: build `zeroqn/crun` branch `fix-passt-net` with this repo's libkrun
  override, krun handler support, and `pkgs.passt` on crun's runtime `PATH`
  for default `krun.use_passt=1` passt/libkrun networking.
- `.#podman`: build Podman against the custom crun so libkrun task runs inherit
  the flake-provided crun/passt runtime path.
- `.#container`: Podman image archive.

---

## Quick start

Show CLI help:

```bash
nix develop --command cargo run -- --help
```

Build image + binary, then run:

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

---

## Runtime modes

### 1) Rootless sidecar mode (default)

Run:

```bash
./result/bin/agentbox
```

What it does (high level):

1. Resolves the selected image and mounts its filesystem.
2. Uses image `/nix` as `lowerdir` for host `fuse-overlayfs`.
3. Builds external merged nix tree under project state.
4. Starts/reuses a deterministic `nix-daemon` sidecar and preserves that
   sidecar while matching task containers are still running.
5. Starts the interactive container with read-only `/nix` + daemon socket.
6. When the last matching task container exits, removes the idle sidecar and
   unmounts the `nix-merged` FUSE overlay so `fuse-overlayfs` does not linger.

Overlay writes live in `<state-root>/nix-upper`; `nix-merged` is only the
mounted merged view and may be unmounted/recreated between runs.

Sidecar metadata is saved at:

```text
<state-root>/nix-sidecar.state
```

Disable sidecar mode for one native task run:

```bash
./result/bin/agentbox --task-native --disable-nix-sidecar
```

Or disable the sidecar globally when also opting into native task runtime:

```bash
AGENTBOX_NIX_SIDECAR=0 ./result/bin/agentbox --task-native
```

---

### 2) Seeded mode (legacy fallback)

First run copies image `/nix/store` and `/nix/var/nix` into project state,
then reuses that data across runs.

Use seeded mode:

```bash
./result/bin/agentbox --task-native --disable-nix-sidecar
# or
AGENTBOX_NIX_SIDECAR=0 ./result/bin/agentbox --task-native
```

State layout:

```text
<state-root>/
  cargo/
  nix/
    .seeded
    store/
    var/
      log/
        nix/
      nix/
```

If partial seed data exists without `.seeded`, `agentbox` treats it as
inconsistent and refuses to auto-seed.

---

### Default libkrun task runtime

Run the interactive task container with crun's libkrun handler:

```bash
./result/bin/agentbox
```

Use libkrun TSI networking instead of default passt networking:

```bash
./result/bin/agentbox --tsi
```

Set libkrun VM memory explicitly in integer GiB:

```bash
./result/bin/agentbox --mem 8
```

When `--mem` is omitted, default libkrun mode uses 80% of detected host memory,
rounded down to a whole GiB, and passes that value to libkrun as MiB. For
example, a 10 GiB host produces `krun.ram_mib=8192`.

Default libkrun mode does not pin CPUs. On Linux hosts with 6 or fewer available
CPUs, `agentbox` passes all available CPUs to KVM with `krun.cpus=<available>`.
On larger Linux hosts, it reserves two CPUs for the host and passes
`krun.cpus=available_cpus - 2`.

Run the interactive task container with normal native Podman instead:

```bash
./result/bin/agentbox --task-native
```

`--tsi` is libkrun-only; with `--task-native` it parses but has no effect.
`--mem` is also libkrun-only; `--task-native --mem <GiB>` fails fast instead of
configuring native Podman memory.

Default libkrun mode adds the following Podman arguments to the task container
only:

```text
--env AGENTBOX_KVM_DROP_TO_DEV=1
--tmpfs /home/dev/.config:rw,exec,uid=1000,gid=1000,mode=700
--tmpfs /home/dev/.local:rw,exec,uid=1000,gid=1000,mode=700
--tmpfs /home/dev/.cache/starship:rw,exec,uid=1000,gid=1000,mode=700
--tmpfs /home/dev/.cache/tmp:rw,exec,uid=1000,gid=1000,mode=700
--runtime crun --annotation run.oci.handler=krun --annotation krun.ram_mib=<MiB> [--annotation krun.cpus=<count>] --annotation krun.use_passt=1
```

With `--tsi`, `agentbox` omits the passt annotation and adds the `all_proxy=1`
Nix network detection workaround:

```text
--env all_proxy=1 --runtime crun --annotation run.oci.handler=krun --annotation krun.ram_mib=<MiB>
```

The native `nix-daemon` sidecar remains the only Nix daemon authority. Sidecar
containers, sidecar health probes, image mounts, and cleanup probes remain
normal native Podman containers; they do not receive the libkrun task runtime
arguments or the task-only `AGENTBOX_KVM_DROP_TO_DEV` marker.

KVM guests do not share the native Podman user namespace boundary in the same
way as a normal rootless container. If libkrun starts the interactive task shell
as root, the image entrypoint uses the task-only marker or an interactive
`fish -l` task command to drop to the bundled `dev` identity (`1000:1000`)
before starting the shell. Native task containers keep the existing dynamic
`--userns=keep-id` behavior, and the root-required sidecar keeps running as
root. The task command also overlays fish/starship mutable state with tmpfs
mounts under `/home/dev/.config`, `/home/dev/.local`,
`/home/dev/.cache/starship`, and `/home/dev/.cache/tmp`, because libkrun can
expose image-backed home directories as owned by `dev` while still rejecting
writes from the dropped `dev` shell.

Because this behavior is split between task launch arguments and the image
entrypoint, rebuild the `agentbox` binary after changing task arguments and
rebuild/load the container image after changing entrypoint behavior:

```bash
nix build .#container
podman load -i result
```

Default libkrun task runtime requires sidecar mode. Use `--task-native` for
seeded mode:

```bash
./result/bin/agentbox --task-native --disable-nix-sidecar
AGENTBOX_NIX_SIDECAR=0 ./result/bin/agentbox --task-native
```

Direct sharing of the native sidecar Unix socket into a libkrun VM is not
assumed to work. Libkrun mode points the guest at the native sidecar's TCP proxy.
By default it enables libkrun passt networking with `krun.use_passt=1`; `--tsi`
instead sets `all_proxy=1` so Nix detects network availability via its
proxy-environment check. Nix commands inside the KVM guest must still be
validated before claiming success on a host Podman/crun/libkrun stack.

Suggested manual validation:

```bash
# Use the intended host Podman stack, for example after installing .#podman.
podman run --runtime crun --annotation run.oci.handler=krun --annotation krun.use_passt=1 <image> true
./result/bin/agentbox
./result/bin/agentbox --tsi
./result/bin/agentbox --mem 8
./result/bin/agentbox --task-native
# inside the task shell:
id -u
id -g
nix store ping
nix path-info <deterministic-existing-store-path>
nix build nixpkgs#hello --no-link
```

If these Nix commands fail, record the host details and do not treat the mode as
a working KVM Nix setup. The guest-to-host proxy forwards guest requests to a
native host-side daemon authority, so transport and security-boundary changes
must be documented explicitly.

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

It runs with `--userns=keep-id` so `/workspace` ownership matches host mapping.

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
