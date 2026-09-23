# loftd

`loftd` is a Rust CLI that launches a direct-libkrun microVM task environment
from an OCI image. It mounts the current working directory at `/workspace`,
prepares persistent cache disks for `/nix` and rootless container storage, and
enters the guest through the `loftd-guest-init` bootstrap binary.

Loftd owns only the direct-libkrun microVM runtime: the host run path does not
use Podman, crun, or runc. Buildah remains the image-source mechanism for
resolving and refreshing OCI images, and the guest environment still provides
rootless Podman tooling for development.

---

## Prerequisites

- Linux with KVM available to the user running `loftd`.
- `buildah` for loftd's default `btrfs-snapshot` Buildah image-source
  transaction. Image ingestion is rootless and runs as one `buildah unshare`
  transaction so Buildah storage, mount, snapshot, and cleanup share the same
  user namespace.
- `btrfs`, `mkfs.btrfs`, and `blkid` on the host for btrfs-snapshot task-rootfs
  materialization, persistent raw-image creation, and reuse validation
  (`btrfs-progs` + `util-linux`; included in `nix develop` and the Nix
  `.#loftd` helper dir). Task-rootfs btrfs snapshot and delete commands run
  through `buildah unshare`. Rootless btrfs-snapshot cleanup also requires the
  backing btrfs mount to allow user-owned subvolume removal; add
  `user_subvol_rm_allowed` to that mount's options when using this fast path.
- `libkrun.so` at runtime. The Nix `.#loftd` source package keeps `bin/loftd` as
  a raw ELF and resolves libkrun from `$out/lib/loftd` before falling back to
  sonames. The pinned `libkrun` also carries an `$ORIGIN` runpath, so its own
  `libkrunfw.so.5` dlopen resolves against the same `$out/lib/loftd` directory
  instead of an ambient `LD_LIBRARY_PATH`. Source/debug builds can set
  `LOFTD_LIBKRUN_LIBRARY=/path/to/libkrun.so.1`.
- `pasta`/`passt` for host-alias networking in both default passt and opt-in
  `--tsi` mode; included in the Nix `.#loftd` helper dir, `.#loftd-prebuilt`,
  and `nix develop` environments.
- Optional `loftd --pulse=tcp:IP:PORT` audio requires a host Pulse-compatible
  TCP listener, typically provided by `pipewire-pulse`. Loftd exports the
  endpoint to guest PulseAudio-compatible clients but does not configure or
  start the host service.
- `loftd --gpu=drm` exposes a Venus Vulkan device to the guest through the
  libkrun virtio-GPU DRM node. A standalone `virgl_render_server` runner process
  is forked by the loftd launcher with its own Landlock and seccomp sandbox and
  renders Vulkan on the host via RADV against `/dev/dri`; the guest command sees
  a Vulkan device backed by the host GPU. The render-server child reads
  `LOFTD_MESA_LIBDIR`, `LOFTD_MESA_ICD`, and `LOFTD_VULKAN_LOADER_LIBDIR` from
  the caller's environment; the `.#loftd-prebuilt` wrapper sets them, so a bare
  `.#loftd` `bin/loftd` run must export them for `--gpu=drm`. This mode requires
  a libkrun build with `krun_set_gpu_options3` support.
- `loftd --wayland` enables guest Wayland passthrough through
  `wl-cross-domain-proxy` and libkrun virtio-gpu DRM. The loftd image includes
  the guest proxy binary and guest-init exports `XDG_RUNTIME_DIR=/run/user/<uid>`
  plus `WAYLAND_DISPLAY=wayland-0` before the task command starts. This mode
  requires a libkrun build with `krun_set_gpu_options3` support; `--wayland`
  automatically selects `--gpu=drm`.
- `loftd [--workspace=WORKSPACE] --waypipe[=SOCKET] [-- COMMAND...]` launches a
  Waypipe-capable task. An optional absolute SSH-forwarded Unix `SOCKET`
  activates the initial target; valueless `--waypipe` starts the capability
  without a target. Later, `loftd --waypipe exec TASK -- COMMAND...` reuses the
  running server, while `loftd --waypipe=SOCKET exec TASK -- COMMAND...`
  replaces the target and restarts the server before running the command.
  Restarting drops existing GUI applications connected to that Waypipe display.
  Without `--gpu=drm`, Waypipe uses `--no-gpu`; OpenGL/EGL clients use Mesa
  llvmpipe and Vulkan clients use Mesa lavapipe on the guest CPU. With
  `--gpu=drm`, Waypipe keeps GPU support enabled and clients inherit the DRM
  Mesa environment. Waypipe remains mutually exclusive with `--wayland` and
  requires the loftd image's guest `waypipe` binary.
- Linux Landlock enabled in the host kernel for default `loftd` task launches.
  Ordinary launches use host-side Landlock `relax` mode by default; use
  `--landlock=all` for stricter TCP bind handling,
  `--landlock=best-effort` on older/degraded kernels, or `--landlock=off` as an
  explicit debugging escape hatch.
- The packaged default seccomp policy at
  `$out/share/loftd/seccomp/default.json` for ordinary `loftd` task launches
  that omit `--seccomp`; source-built and prebuilt loftd packages install this
  file.
- `strace` for explicit `loftd --seccomp=audit:<trace>` policy-discovery runs.
  It is included in the Nix `.#loftd` helper dir and `nix develop` environments.
  Audit mode uses ptrace on the loftd VM worker only; normal child tracing
  should work with `kernel.yama.ptrace_scope=1`, but hosts that disable ptrace
  entirely must allow ptrace for the audit run.
- `/dev/net/tun` when a guest mode needs TUN-backed networking.

---

## Development

```bash
nix develop
cargo build
cargo test
```

`nix develop` opens `fish` + `starship` by default. Keep your current shell:

```bash
LOFTD_DISABLE_AUTO_FISH=1 nix develop
```

Inside the loftd image, `nix` is invoked through a small compatibility wrapper
that clears the entrypoint's NSS wrapper preload before running the real Nix
binary. This prevents nested dev shells from mixing the container NSS preload
with a different glibc from the shell's realized dependencies.

The container defaults Nix-linked dynamic binaries to `mimalloc` through
`/etc/ld-nix.so.preload`, matching NixOS' allocator preload mechanism rather
than setting a global allocator `LD_PRELOAD`. Select the loftd task allocator
with:

```bash
loftd --alloc=mimalloc
loftd --alloc=hardened
loftd --alloc=glibc
```

`mimalloc` is the default. `hardened` selects GrapheneOS `hardened_malloc`.
`glibc` empties `/etc/ld-nix.so.preload`, so Nix-linked dynamic applications use
glibc's standard allocator without requiring a per-command `bwrap` wrapper. The
image records the mimalloc and hardened_malloc paths in
`/etc/nix-allocator-libs`; the host passes only the allocator mode selector.
`rustc` and `rust-analyzer` are started through wrappers that mask
`/etc/ld-nix.so.preload` for those processes, which remains useful in mimalloc
and hardened modes and is redundant in glibc mode.

Foreign/FHS glibc binaries usually read `/etc/ld.so.preload` instead of
`/etc/ld-nix.so.preload`, while static or musl binaries generally ignore both
files. For a specific foreign/FHS command, opt in to GrapheneOS
`hardened_malloc` with:

```bash
hardening-run some-foreign-binary --flag
```

`hardening-run` sets `LD_PRELOAD` only for the wrapped command. The in-image
`loftd-guest-init` binary is the static musl bootstrap path that materializes
the selected preload file; dynamic `--guest-init` overrides are not guaranteed
to run under GrapheneOS `hardened_malloc` until after they have started and
rewritten `/etc/ld-nix.so.preload`. The usual opt-out remains:

```bash
env -u LD_PRELOAD some-foreign-binary --flag
```

---

## Build

```bash
nix build .#loftd
nix build ./nix/dev#loftd-dev
nix build .#loftd-prebuilt
nix build .#loftd-musl
nix build .#rmux-prebuilt
nix build .#rtk-prebuilt
nix build .#herdr-prebuilt
nix build .#dolt-prebuilt
nix build .#beads-prebuilt
nix build .#monty-prebuilt
nix build .#libkrunfw
nix build .#libkrun
nix build .#crun
nix build .#podman
nix build .#container-lib-policy-seccomp-json
nix build .#container
```

CI publishes loftd release artifacts on every push to `main` and on every git
tag (`v*`):

- **Rolling** (branch push to `main`): `loftd-<arch>-unknown-linux-gnu` is
  uploaded to the `alpha` prerelease and to a `sha-<12chars>` immutable
  prerelease.
- **Versioned** (tag push, e.g. `v0.1.0`):
  `loftd-<version>-<arch>-unknown-linux-gnu` is uploaded to a full
  (non-prerelease) release named after the tag, and to the matching
  `sha-<12chars>` immutable prerelease.
- **Images** (`ghcr.io/<owner>/loftd:<tag>`) are published by the image workflow
  on every push to `main` (`latest`, `sha-<12chars>`), every push to `dev`
  (`dev`, `sha-<12chars>`), and every tag push (the tag name itself, plus
  `sha-<12chars>`).

See [Build outputs and Nix store/DB diagnostics](docs/build.md) for what each
flake output produces and how the image Nix DB metadata checks work.

---

## Quick start

Show the CLI help:

```bash
nix develop --command cargo run -p loftd -- --help
```

Build the image and the loftd binary, then load the image and run a task:

```bash
nix build .#container
podman load < result
nix build .#loftd
./result/bin/loftd -- bash -lc 'echo ok'
```

Image selection behavior:

- default: `localhost/loftd:latest`
- fallback: `ghcr.io/zeroqn/loftd:latest`

Force GHCR latest:

```bash
./result/bin/loftd --pull-latest
```

Override the image explicitly:

```bash
LOFTD_IMAGE=<image-ref> ./result/bin/loftd
# or
./result/bin/loftd --image <image-ref>
```

Enter the final task shell as root when root-only operations are needed:

```bash
./result/bin/loftd --root -- bash -lc 'id'
```

By default, loftd drops the interactive shell to the host/dev identity. `--root`
is an explicit opt-in that keeps only the final task shell/command as root
inside the guest; it does not install or require `sudo`.

Collect loftd component timings:

```bash
./result/bin/loftd --profile --debug -- bash -lc 'echo ok'
```

`--profile` enables timing collection. Timings are printed only when `--debug`
is also set, and reports are written to stderr so stdout remains reserved for
command output.

---

## Usage

`loftd` builds a typed launch plan, uses Buildah as the durable OCI image source
for the default btrfs path, materializes a per-task btrfs snapshot rootfs,
prepares loftd-owned persistent raw btrfs disks for `/nix` and rootless
container storage, starts a same-binary helper through a strict keep-id
`unshare` wrapper around `<loftd-exe> internal libkrun-network-enter
<launch.conf>` to set up the per-session pasta namespace and call libkrun, and
enters the guest through `loftd-guest-init enter`. Interactive runs are managed
by a guest-side PTY session manager, so the host terminal is an attach client
rather than the lifetime owner of the guest shell or terminal command. The
helper owns final cleanup for managed sessions; `loftd kill` remains the
recovery path for detached tasks, and `--preserve-debug` keeps task state for
manual inspection. Managed attach sockets are runtime-only host sockets under
`/tmp/loftd-<uid>/`; the active-task record stores the exact socket path for
`loftd attach`, and helper cleanup removes only the current task's socket. The
explicit `fuse-overlay` backend is still a future slice.

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
./result/bin/loftd --new-perms=io-uring -- bash -lc 'echo ok'
./result/bin/loftd --tsi -- bash -lc 'echo ok'
./result/bin/loftd --tsi --pulse=tcp:127.0.0.1:4713 -- bash -lc 'printf "%s\n" "$PULSE_SERVER"'
./result/bin/loftd --pulse=tcp:192.0.2.10:4713 -- paplay sample.wav
./result/bin/loftd --profile -- bash -lc 'echo ok'
./result/bin/loftd --guest-init ./result-musl/bin/loftd-guest-init -- bash -lc 'echo ok'
./result/bin/loftd -- bash -lc 'echo ok'
./result/bin/loftd --workspace=/home/dev/foo --waypipe
./result/bin/loftd --workspace=/home/dev/foo --waypipe=/tmp/loftd-waypipe.sock -- gui-application
./result/bin/loftd ps
./result/bin/loftd exec <task-id-or-handle-selector> -- bash -lc 'echo ok'
./result/bin/loftd --waypipe exec <task-id-or-handle-selector> -- gui-application
./result/bin/loftd --waypipe=/tmp/other-waypipe.sock exec <task-id-or-handle-selector> -- gui-application
./result/bin/loftd attach <task-id-or-handle-selector>
./result/bin/loftd a <task-id-or-handle-selector>
./result/bin/loftd kill <task-id-or-handle-selector>
./result/bin/loftd container-store resize --size 128G
./result/bin/loftd container-store reset --force
```

`loftd exec`, the session lifecycle, and the rest of the day-to-day reference
live in the topic docs:

- [Usage reference](docs/usage.md) — `loftd exec`, detach/attach sessions,
  task control (`ps`/`kill`), volumes, the root shell handoff, container-store
  maintenance, and guest-init overrides.
- [Graphics and audio](docs/graphics-audio.md) — Pulse TCP audio, remote
  Waypipe, GPU, and Wayland passthrough.
- [Security](docs/security.md) — host Landlock, host seccomp policies, guest
  permissions, and the packaged nested-container seccomp policy.
- [Networking](docs/networking.md) — passt/TSI network modes, host aliases,
  published ports, and nested virtualization.
- [Images and storage](docs/images-and-storage.md) — image selection and
  cache management, task-rootfs backends, the `/nix` host overlay, the
  container-store disk, guest memory and zram swap, and launch config keys.
- [Diagnostics](docs/diagnostics.md) — log levels, fd-pressure reports,
  profiling, terminal tracing, the PTY benchmark, and troubleshooting.
- [Internals](docs/internals.md) — libkrun and host-tool lookup, the prepared
  root, and the guest entry contract.

---

## Persistent host mounts

Each run ensures these host-backed paths and grafts them into the prepared root:

- current workspace -> `/workspace`
- `~/.codex` -> `/home/dev/.codex`
- `~/.omp` -> `/home/dev/.omp`
- `~/.pi` -> `/home/dev/.pi`
- `~/.local/share/cortexkit` -> `/home/dev/.local/share/cortexkit`
- `~/.config/dirge` -> `/home/dev/.config/dirge`
- `~/.local/share/dirge` -> `/home/dev/.local/share/dirge`
- `~/.dirge` -> `/home/dev/.dirge`
- `<state-root>/cargo` -> `/home/dev/.cargo`
- `<loftd-state>/sccache` -> `/home/dev/.cache/sccache`
- each `-v, --volume SOURCE:TARGET[:ro|:rw]` -> the requested absolute `TARGET`

This keeps tool config/state and compiler-cache state outside the repo while
matching the task-volume contract.

---

## State root and config

Default state root:

```text
$XDG_STATE_HOME/loftd/<repo-slug>
```

Fallback when `XDG_STATE_HOME` is unset:

```text
$HOME/.local/state/loftd/<repo-slug>
```

Override base location in:

```text
$XDG_CONFIG_HOME/loftd/loftd.toml
```

or:

```text
$HOME/.config/loftd/loftd.toml
```

Example:

```toml
[state]
location = "/home/dev/xxx/"
```

This makes the base `/home/dev/xxx/loftd`.

Loftd also keeps a shared sccache at:

```text
<state.location>/loftd/sccache
```

That directory is bind-mounted into each task container at
`/home/dev/.cache/sccache`, so compiler cache entries are reused across
loftd repos and containers.

---

## Container environment summary

The container provides:

- interactive `fish` + `starship`
- bubblewrap (`bwrap`) and Pi (`pi`)
- the `sqlite3` CLI in the agent layer, so agent tooling can repair a corrupted
  local database (for example Magic Context's)
- the pinned monty worker (`monty`, `MONTY_BIN`) that backs the RLM extension's
  sandboxed Python kernel
- cargo-deny and Symposium (`cargo-agents`, invoked as `cargo agents`)
- Python 3 (`PyYAML`, Tree-sitter, Tree-sitter Rust parser), Node.js
- Rust toolchain (`cargo`, `rustc`, `clippy`, `rustfmt`, `rust-analyzer`, `sccache`, `mold`)
- `gcc`, `musl`, `clang`
- `mimalloc` enabled by default for Nix-linked dynamic binaries through `/etc/ld-nix.so.preload`; loftd selects the task allocator with `--alloc=mimalloc`, `--alloc=hardened`, or `--alloc=glibc`, and `hardening-run` remains the per-command foreign/FHS `LD_PRELOAD` opt-in
- RTK (`rtk`)
- libkrun 1.18.0 (`libkrun.so`) plus pinned `libkrunfw.so` for nested KVM support inside the container
- `nix` wrapper that clears the container NSS wrapper preload before invoking
  the real Nix binary, avoiding glibc-version mismatches in nested dev shells
- `loftd-nix-store-db-check` for non-mutating live `/nix/store` vs Nix DB
  validity diagnostics, including cautious libkrun upperdir store-layer
  evidence when `/run/loftd/nix-disk/upper` is visible
- `rustc` and `rust-analyzer` wrappers that mask `/etc/ld-nix.so.preload` so
  both tools keep the default allocator
- `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER` preset to the bundled
  `clang_mold_wrapper` helper for the `x86_64-unknown-linux-gnu` target
- `LIBCLANG_PATH` preset to the bundled Nix `libclang` library directory
- `RUSTC_WRAPPER`, `CMAKE_C_COMPILER_LAUNCHER`, and `CMAKE_CXX_COMPILER_LAUNCHER` preset to the bundled `sccache`
- `SCCACHE_DIR=/home/dev/.cache/sccache`, backed by the shared host cache under the loftd state root
- `/usr/bin/env` compatibility for common env-based shebangs such as
  `#!/usr/bin/env bash`
- narrow hardcoded-interpreter compatibility for `/bin/sh`, `/bin/bash`,
  `/bin/python`, and `/bin/python3`; `/bin/python` resolves to Python 3
  (not broad FHS compatibility)
- common tools (`curl`, `jq`, `openssl`, `tmux`, `rmux`, etc.); `tmux` comes
  from Nixpkgs in the loftd image, and the pinned `rmux`
  release remains available separately as `rmux`. `/etc/rmux.conf` is the
  image-level rmux config path. In the loftd image, the default config disables
  mouse mode for native terminal selection, binds `T` to toggle mouse mode,
  keeps large history and one-based window/pane indexes, uses vi keys, and
  creates splits and new windows in the current pane directory.

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

On push to `main`, push to `dev`, and tag pushes, CI publishes the loftd image:

- `ghcr.io/<repo-owner>/loftd:latest` (main only)
- `ghcr.io/<repo-owner>/loftd:dev` (dev only)
- `ghcr.io/<repo-owner>/loftd:<git-tag>` (tag only)
- `ghcr.io/<repo-owner>/loftd:sha-<12-char-commit>`

The image is built from `.#container` and verifies `loftd-guest-init`.

### Prebuilt binaries (GitHub Releases)

Main-branch CI also publishes prerelease binary assets:

- rolling `alpha`
- commit-specific `sha-<12-char-commit>`

Older `sha-*` prereleases are pruned (retains newest 20).

The `loftd-<arch>-unknown-linux-gnu` asset is a neutral dynamic Linux ELF
packaging input and intentionally non-standalone: it must not contain
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

The pinned-asset refresh scripts for `nix/pins.nix` (loftd, RTK, rmux, dolt,
beads, monty, libkrun, libkrunfw, Pi, dirge, omp, herdr, and zvec-grep) are
documented in [docs/maintenance.md](docs/maintenance.md).

---

## Use from another flake (prebuilt binary)

```nix
{
  inputs.loftd.url = "github:zeroqn/agentbox";

  outputs = { self, nixpkgs, loftd, ... }: {
    nixosConfigurations.my-host = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ({ pkgs, ... }: {
          environment.systemPackages = [
            loftd.packages.${pkgs.system}.loftd-prebuilt
          ];
        })
      ];
    };
  };
}
```

For a source-build fallback, use:

```nix
loftd.packages.${pkgs.system}.loftd
```

Downstream flakes that install `.#loftd` or `.#loftd-prebuilt` receive the
loftd host-side default policy at:

```text
$out/share/loftd/seccomp/default.json
```

Downstream flakes can also depend on the separate packaged guest/container
seccomp policy via:

```nix
loftd.packages.${pkgs.system}.container-lib-policy-seccomp-json
```
