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

### Build outputs

- `.#loftd`: compile the workspace Rust host package with `$out/bin/loftd` as a
  raw dynamic ELF. Runtime helpers are installed under
  `$out/libexec/loftd-helpers`, and the shared `libkrun`/`libkrunfw` packages
  are exposed under `$out/lib/loftd`, so source-built loftd needs no wrapper
  script or duplicate payload.
- `./nix/dev#loftd-dev`: local-checkout-only development build of the workspace
  Rust host package wired to the checked-out `deps/libkrun` and `deps/libkrunfw`
  submodules through the submodule-aware dev flake. Use this target for local
  libkrun/libkrunfw or kernel configuration experiments; downstream flakes that
  consume this repository via `github:` should use non-dev root outputs.
- `.#loftd-prebuilt`: install a pinned published neutral dynamic Linux `loftd`
  asset as raw `$out/bin/loftd`, patch ordinary ELF runtime dependencies with
  Nix, and provide the same package-relative helper and `$out/lib/loftd`
  library layout as source-built `.#loftd`.
- `.#loftd-musl`: static/musl `loftd-guest-init` (and `loftd-granted`) binaries
  for image/guest use. It intentionally does not build or expose `bin/loftd`;
  the host `loftd` binary is always dynamically linked so it can load
  `libkrun.so`/`libkrunfw.so` from the package or dev shell runtime library
  path.
- `.#rmux-prebuilt`: install the pinned published Helvesec/rmux Linux release
  tarball for the current system. The loftd image includes this package as
  `rmux` alongside Nixpkgs `tmux`.
- `.#rio-bin` (`x86_64-linux`): install the pinned `zeroqn/headless` Rio package.
  The x86_64 loftd image includes `rio` and installs its `rio` and `xterm-rio`
  terminfo entries in `/home/dev/.terminfo`, so managed guest shells can use
  either Rio terminal identity without additional guest setup. Upstream does
  not currently publish this package for `aarch64-linux`.
- `.#rtk-prebuilt`: install the pinned published RTK release asset (currently
  pinned for `x86_64-linux`).
- `.#herdr-prebuilt`: install the pinned published `herdrdev/herdr` Linux
  release binary (static-PIE) for the current system. The loftd image includes
  this package as `herdr` in the agent layer.
- `.#zvec-grep` (`x86_64-linux`): install the pinned `zvec-ai/zvec-grep` (`zg`)
  hybrid workspace search CLI from the GitHub source archive, wrapped around
  Nixpkgs Node.js. The agent layer keeps the glibc x86_64 native payloads and
  prunes the musl, CUDA, and cross-arch copies the image cannot load.
- `.#dolt-prebuilt`: install the pinned `dolthub/dolt` Linux release tarball
  binary for the current system. The loftd image includes this package as
  `dolt` in the agent layer.
- `.#beads-prebuilt`: install the pinned `gastownhall/beads` Linux release
  tarball binary for the current system, patched with Nix to use the image's
  glibc and libstdc++. The loftd image includes this package as `bd` in the
  agent layer.
- `.#monty-prebuilt` (`x86_64-linux`): install the pinned published
  `@pydantic/monty-linux-x64-gnu` npm tarball's `monty` worker (the sandboxed
  Python interpreter the RLM extension spawns), patched with Nix to use the
  image's glibc and libstdc++. The image includes this package as `monty` in
  the agent layer and exports `MONTY_BIN` pointing at it, so the extension uses
  the store worker instead of the platform package it may find in
  `node_modules`. Pin the version in lockstep with the `@pydantic/monty` JS
  client: client and worker reject each other over a protocol-version mismatch.
- `.#libkrunfw`: install the pinned `zeroqn/libkrunfw` release asset for the
  current system.
- `.#libkrun`: install the pinned `zeroqn/libkrun` `loftd-*` prebuilt release
  asset for the current system, matching `.#libkrunfw`'s release-asset model.
  Root consumers (`.#crun`, `.#podman`, `.#loftd`, images, and
  `.#loftd-prebuilt`) all use this pinned prebuilt package. The package
  normalizes upstream Linux `lib64` payloads into `$out/lib` and regenerates
  `libkrun.pc` for the Nix store path. Local source development for libkrun is
  intentionally limited to the submodule-aware dev flake (`./nix/dev#loftd-dev`).
- `.#virglrenderer`: the nixpkgs `virglrenderer` with this repo's host-side
  patches (`virglrenderer-enum-26.patch` and
  `virglrenderer-gbm-layout-linear-modifier.patch`, applied by the overlay in
  `nix/lib/systems.nix`). Host-side only: libkrun links `libvirglrenderer.so.1`
  and the `virgl_render_server` helper is symlinked from this package, so the
  loftd packages already ship it; downstream flakes that build their own host
  vrend/libkrun stack should consume this output instead of nixpkgs'
  `virglrenderer`.
- `.#crun`: build `zeroqn/crun` branch `agentbox` with this repo's libkrun
  override, krun handler support, raw data disk annotation support,
  `krun.nested_virt` support, and `pkgs.passt` on crun's runtime `PATH`.
- `.#podman`: build Podman against the custom crun for libkrun/raw-image
  development.
- `.#container-lib-policy-seccomp-json`: install the pinned
  `containers/container-libs` `common/pkg/seccomp/seccomp.json` policy at
  `share/containers/seccomp.json` for downstream flakes or image reuse.
- `.#container`: loftd Podman image archive named `localhost/loftd:latest`;
  includes rootless Podman tooling such as Podman, Buildah, crun, netavark,
  aardvark-dns, passt, and docker-compose, and Nix formatting tooling such as
  `nixfmt`.

### Nix store / DB diagnostics

`nix build .#container` depends on a static image metadata linter before running
the layered image build command. To run only that linter:

```bash
nix build .#checks.$(nix eval --raw --impure --expr builtins.currentSystem).container-nix-db-metadata
```

The check compares store paths referenced by the image Docker config/env against
the `pkgs.closureInfo { rootPaths = layers.imageContents; }` store-path list.
That is the same closure Docker Tools loads into the image Nix DB when
`includeNixDB = true`. It fails fast when image metadata can pull a store path
into `/nix/store` without that path being covered by generated image Nix DB
metadata. This check does not inspect or mutate the host Nix DB.

Inside a loftd container, run the packaged live DB scanner manually:

```bash
loftd-nix-store-db-check
```

The runtime checker compares present `/nix/store/<hash>-name` entries with
`nix path-info --all`, ignores the internal `/nix/store/.links` link farm and
transient `*.lock` files, and prints `nix-store --verify-path` evidence for
present-but-invalid paths. When the libkrun Nix disk upperdir is visible at
`/run/loftd/nix-disk/upper`, failures also compare each invalid store object
with `/run/loftd/nix-disk/upper/store/<name>` and report whether that store-layer
object is present in the upperdir or not found there. This is store-layer
evidence only, not root-cause proof: absence from the upperdir is not proof that
lower image metadata is correct or that the lower image is at fault. It is
diagnostic only and never repairs or mutates the Nix DB.

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

`loftd exec <task-id-or-handle-selector> -- COMMAND...` runs a non-PTY foreground
command in an active task with separate stdin, stdout, and stderr streams. It
uses the task's `/workspace` directory and returns the guest command's exit
status. Tasks launched by older loftd versions do not have the exec transport;
relaunch them with the current version before using `loftd exec`.

Pulse TCP audio:

```ini
# ~/.config/pipewire/pipewire-pulse.conf.d/loftd-tcp.conf
pulse.properties = {
    server.address = [
        "unix:native"
        "tcp:127.0.0.1:4713"
    ]
}
```

Restart the host `pipewire-pulse` user service after adding the drop-in. Use
`--pulse=tcp:localhost:PORT` or `--pulse=tcp:127.0.0.1:PORT` to create a
private per-task bridge to the selected host IPv4-loopback listener. The guest
receives `PULSE_SERVER=unix:/run/user/<uid>/loftd-pulse`; each guest Pulse
connection crosses a dedicated libkrun vsock channel and connects only to the
configured host `127.0.0.1:PORT`. Loftd does not enable passt host-loopback
mapping or expose other host-loopback ports. Task launch succeeds if the host
listener is unavailable, and a later guest connection can succeed after the
host service starts or restarts.

Other literal IPv4 and bracketed IPv6 endpoints remain direct guest TCP
endpoints and are exported canonically as `PULSE_SERVER=tcp:IP:PORT`. Loftd
does not start host or guest `pipewire`/`pipewire-pulse`, proxy native PipeWire
`pipewire-0`, forward a Pulse cookie, or test the connection before launch.
`loftd exec` inherits the endpoint selected when the task was launched and
cannot change it.

Host `pipewire-pulse` configuration owns authorization. A permitted Pulse
client may gain playback, capture, stream inspection, or server-control access,
so configure the listener's access policy for the trust granted to the task.
For combined software-only Waypipe playback, mpv 0.41.0 also needs
`--gpu-sw=yes`, for example:

```bash
./result/bin/loftd \
  --waypipe=/tmp/loftd-waypipe.sock \
  --pulse=tcp:localhost:4713 \
  -- mpv --gpu-sw=yes '/workspace/'*.mp4
```

Remote Waypipe launch:

```bash
# Workstation: connect Waypipe to the local compositor.
waypipe --socket "$XDG_RUNTIME_DIR/loftd-waypipe.sock" client

# Workstation: keep an authenticated reverse Unix-socket forward open.
ssh -R /tmp/loftd-waypipe.sock:"$XDG_RUNTIME_DIR/loftd-waypipe.sock" loftd-host

# loftd host: launch a Waypipe-capable task with the initial target.
./result/bin/loftd \
  --workspace=/home/dev/foo \
  --waypipe=/tmp/loftd-waypipe.sock \
  -- gui-application

# Reuse the running Waypipe server for another GUI command.
./result/bin/loftd --waypipe exec TASK -- another-gui-application

# Replace the target, restart the guest Waypipe server, then run a command.
./result/bin/loftd --waypipe=/tmp/other-waypipe.sock exec TASK -- gui-application
```

loftd validates that the selected workspace is an absolute directory and each
valued socket path is absolute and already exists as a Unix socket. The guest
command is optional; when omitted, loftd starts the normal interactive fish
login shell. Valueless `--waypipe` launches the task capability without an
active target. A valueless Waypipe exec reuses the running server and display.
A valued Waypipe exec serially changes the target, terminates and reaps the
running server, starts a fresh server on the stable display name, waits for
readiness, and only then starts the command. This is replacement, not protocol
reconnection: existing GUI applications connected to the old server lose their
Wayland connection and normally exit. If `--workspace` is omitted, loftd uses
the current working directory. loftd does not start SSH or the workstation
Waypipe client and does not create, unlink, or clean up the forwarded socket.
Without `--gpu=drm`, the mode passes `--no-gpu` to Waypipe. The loftd guest
image provides Mesa software rendering for this path: OpenGL/EGL applications
use llvmpipe and Vulkan applications use lavapipe on the guest CPU. When
`--gpu=drm` is also selected, guest-init omits Waypipe's `--no-gpu`, does not
force the software-renderer environment, and preserves the DRM-scoped Mesa
OpenGL/EGL and Vulkan discovery paths. `--waypipe` remains mutually exclusive
with `--wayland`.

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
  environment passthrough. When the managed guest reports an exit status, loftd
  propagates that status as the foreground process result; a guest status such
  as 127 is distinct from loftd helper or VM infrastructure failure diagnostics.
  Managed helper diagnostics are mediated by the parent after terminal raw mode
  is restored, so infrastructure errors start on a fresh terminal line instead
  of racing with guest PTY output. Detached or `--preserve-debug` sessions keep
  helper stderr in the task state directory as `helper.stderr.log` for later
  inspection; normal managed cleanup removes it with the task state.
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
  attach fails clearly instead of falling back to another transport. On slow or
  heavily loaded hosts, managed attach readiness can need a longer guest boot
  window before the guest sends its initial `Hello` frame. Increase the bounded
  readiness windows with:

  ```bash
  LOFTD_MANAGED_ATTACH_READY_TIMEOUT_SECS=180 \
  LOFTD_MANAGED_HELPER_READY_TIMEOUT_SECS=190 \
  ./result/bin/loftd -- bash -lc 'echo ok'
  ```

  `LOFTD_MANAGED_ATTACH_READY_TIMEOUT_SECS` controls how long the helper waits
  for the guest attach listener to complete the initial `Hello` handshake.
  `LOFTD_MANAGED_HELPER_READY_TIMEOUT_SECS` controls how long the parent waits
  for the helper to report readiness.
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

Guest permissions:

```bash
./result/bin/loftd --new-perms=io-uring
./result/bin/loftd --new-perms=perf
./result/bin/loftd --new-perms=io-uring,net-admin,net-raw,bpf,perf,sys-admin
```

- `--new-perms` grants the comma-separated additional permissions `io-uring`, `net-admin`,
  `net-raw`, `bpf`, `perf`, and `sys-admin`. Values are order-independent and duplicates are
  ignored.
  The former `--permissions`, `--io-uring`, and `--perf` flags have been removed.
- No optional permission is enabled by default. Normal initial commands, managed PTY
  commands, hidden `as-dev` commands, and later `loftd exec` commands run without
  effective, permitted, inheritable, or ambient capabilities. Loftd retains only the
  required rootless-ID-map and authorized grant capabilities in the guest bounding set.
- Without `io-uring`, loftd disables creation of new io_uring instances
  guest-wide by setting `kernel.io_uring_disabled=2` during root guest
  initialization. This happens before Nix and Podman preparation, Wayland
  startup, managed-session startup, or the task command. Guest initialization
  fails closed if the sysctl cannot be applied.
- `io-uring` allows processes in the dynamic guest `dev` group to create
  io_uring instances without `CAP_SYS_ADMIN`. Guest-init writes the `dev` GID to
  `kernel.io_uring_group` and keeps `kernel.io_uring_disabled=1`; processes
  outside that group remain denied unless permitted by the kernel's
  `CAP_SYS_ADMIN` exception. `io-uring` itself does not grant `CAP_SYS_ADMIN`;
  an explicit `sys-admin` grant independently satisfies that exception for a
  command launched through `loftd-granted`.
- `net-admin`, `net-raw`, `bpf`, and `sys-admin` authorize `CAP_NET_ADMIN`,
  `CAP_NET_RAW`, `CAP_BPF`, and `CAP_SYS_ADMIN`, respectively, for the explicit
  `loftd-granted COMMAND [ARG ...]` helper. `CAP_SYS_ADMIN` is exceptionally broad;
  it remains absent from normal guest commands and is granted only to commands launched
  through this helper. Every helper invocation receives all capability-bearing permissions
  authorized for the task; the helper refuses to run when none were authorized. For example:

  ```bash
  ./result/bin/loftd --new-perms=sys-admin -- loftd-granted fish
  ```

  The helper is installed root-owned with the exact authorized file capabilities under
  the read-only `/run/loftd/wrappers` tree. It does not read grants from its arguments,
  environment, or a policy file. A capability-bearing subtree still needs to drop its
  capabilities before invoking programs such as Bubblewrap that reject unexpected
  permitted capabilities.
- The loftd guest image includes `perf` and `strace` on `PATH`. Without `perf`,
  loftd leaves the guest kernel's hardened `kernel.perf_event_paranoid=3`
  setting unchanged. `perf` sets `kernel.perf_event_paranoid=-1` and
  `kernel.kptr_restrict=0` before the task starts, enabling unprivileged kernel
  software events, tracepoints, and nonzero `/proc/kallsyms` addresses while
  weakening guest performance-event and kernel-pointer isolation.
- Hardware PMU events such as cycles and instructions are not guaranteed. The
  current x86 libkrun CPUID configuration disables the architectural PMU, so
  software events and available tracepoints are the supported profiling scope.
- These permissions affect only processes inside the guest VM. They do not
  alter loftd's host VM-worker capabilities, host seccomp, host Landlock, or
  host networking.
- Nested Podman capability and seccomp policy remains independent. In
  particular, the packaged nested-container profile blocks io_uring syscalls,
  and selected guest capabilities are not automatically granted inside nested
  containers.

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
memory rounded down to whole GiB, matching the libkrun VM memory policy. Pass
`loftd --mem <GiB>` to override that default. Guest bootstrap also sets
`SCCACHE_DIR=/home/dev/.cache/sccache`, backed by loftd's shared state
`sccache` bind mount.

Guest RAM is fixed for the life of the microVM, so the guest also gets zram
swap: during `enter`, before the shell or any background preparation starts,
`loftd-guest-init` sets the device capacity in `/sys/block/zram0/disksize`,
bounds what the device may spend in `/sys/block/zram0/mem_limit`, signs it with
`mkswap`, and activates it with `swapon -p 100`. The capacity equals guest RAM
and the memory budget is a quarter of it. Capacity counts uncompressed bytes
and zram holds a page compressed, so compressible content costs a fraction of
the space it occupies, while the budget stops pages that do not compress from
being parked at roughly 1:1. The pinned `libkrunfw` kernel is built with
`CONFIG_SWAP` and `CONFIG_ZRAM` (zstd default, lzo available). Swap makes cold
anonymous pages reclaimable, which turns memory pressure into slower progress
instead of a guest OOM kill; it does not add memory, and a kernel without zram
records `state=unavailable` in `/run/loftd/swap.status` rather than failing the
session.

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

For guest-side file-descriptor pressure, `loftd-guest-init fd-report` prints the
worst descriptor consumers in the guest (`pid`, command, count, soft limit, and
an `socket`/`pipe`/`anon_inode`/`regular` target breakdown), the guest-wide
`/proc/sys/fs/file-nr` allocation against `file-max`, and the origin of any
exhaustion it observes. `origin=guest` means a guest-local open failed with
`EMFILE`/`ENFILE`; `origin=host` means a guest-local open succeeded while the
virtiofs probe target `/workspace` failed with `EMFILE`/`ENFILE`, which points at
the host libkrun VM worker that backs every virtiofs mount rather than at the
guest. Pass `--watch` (with `--interval-secs`, default 10) to keep printing a
fresh report. Managed guest sessions sample the same report every 10 seconds
into `/run/loftd/fd-pressure.status` and print a warning to `loftd-guest-init`
stderr (captured in the task's `helper.stderr.log`) when a process crosses
50/75/90% of its soft `RLIMIT_NOFILE` or grows by 256 descriptors between
samples, so pressure is visible before a command fails with `EMFILE`.

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
stdout batch, stdout write, and stdout flush timings. Host `stdout_batches`,
`stdout_batch_frames_*`, and `stdout_batch_bytes_max` describe how many
immediately available guest data frames were coalesced into each host stdout
write/flush; `stdout_write_count` and `stdout_flush_count` are the resulting
terminal write/flush calls. Compare these host counters with host `frames` and
the guest drain counters to see whether output fragmentation was reduced. The
guest summary includes PTY readable events, PTY read sizes, full-buffer read
count, attached-drain/coalescing counters, terminal normalize/parser time, and
guest frame-write time. Guest summaries keep the compatibility
`normalize_parse_total_us` and `normalize_parse_max_us` fields as combined
terminal-processing timings for each forwarded burst. After attached-drain
coalescing, one forwarded burst can contain multiple PTY reads, so these fields
are no longer necessarily one original PTY read. Split `normalize_*` and
`parser_*` fields use the same forwarded-burst basis for latency analysis.
Nonzero `pty_drain_coalesced_*` counters show that immediately available PTY
reads were combined before forwarding; `WouldBlock` is the expected normal
attached-drain exit, and `pty_drain_bound_hit_count` shows when the conservative
drain caps stopped a burst. Attaching to an already-running managed task
profiles the host attach path immediately, but guest-side attach metrics are
available only if that task was originally launched with `LOFTD_ATTACH_PROFILE=1`.

For live-output compatibility diagnostics, pass `--pty=raw` when launching a
new `loftd` task. The default is `--pty=normalize`. This default-off raw mode is
intended for terminal-rendering A/B checks such as comparing a TUI under the
normal managed PTY path versus raw live PTY forwarding. It only changes bytes
sent from the guest PTY to the attached host client: live `Frame::Data` payloads
carry the original PTY bytes, while guest-init still keeps its normalized parser
copy for detach/reattach restore state. It does not change the attach protocol,
stdin forwarding, detached restore frames, or the default behavior. It only
affects newly launched tasks, not `loftd attach` to an existing task.

Add the `trace` token, or set boolean-style `LOFTD_TERMINAL_TRACE=1`, to collect
terminal diagnostics. On the host, trace output writes to
`./loftd-terminal.trace` in the current working directory used for the launch.
Inside the guest, guest-init writes the same workspace-mounted file as
`/workspace/loftd-terminal.trace`. Custom paths are intentionally ignored so the
host and guest stay on that single shared workspace trace file. A new traced
launch truncates the host workspace trace file before appending fresh events.
When a traced data burst contains alternate-screen enter or exit sequences, the
line also includes bounded hex and escaped-byte context around those hits so the
surrounding terminal output can be inspected without dumping the full PTY burst.
For host-to-guest stdin and guest PTY-input bursts that contain ESC, C0 control,
or DEL bytes, the line also includes bounded `input_contexts=` hex and
escaped-byte context. Terminal tracing is opt-in diagnostic output and can
therefore include small bounded snippets of terminal input/control-byte payloads.
The falsey values `0`, `false`, `no`, `off`, and an empty value disable the
environment opt-in. Raw mode and tracing are independent; when `--pty` contains
only modifier tokens such as `trace`, `no-focus-input`, or
`focus-report-guard`, loftd uses the default `normalize` mode. The bounded
focus-report guard is enabled by default and suppresses exact host terminal
focus gained/lost reports (`ESC[I` and `ESC[O`) only during a 750 ms guard after
guest output enables or reasserts focus reporting (`ESC[?1004h`). The guard also
ends early after the first non-focus host input is forwarded. Add
`focus-report-guard` only for explicitness. Add the stronger `no-focus-input`
token to suppress those exact focus reports for the whole initial-launch stdin
path. These input-side diagnostics do not change guest-to-host PTY output,
detached restore frames, or later `loftd attach` sessions.

```bash
loftd --pty=focus-report-guard
loftd --pty=no-focus-input
loftd --pty=trace
loftd --pty=trace,focus-report-guard
loftd --pty=trace,no-focus-input
loftd --pty=normalize,trace
loftd --pty=raw
loftd --pty=raw,trace
loftd --pty=normalize,focus-report-guard,trace
loftd --pty=normalize,no-focus-input,trace
loftd --pty=raw,focus-report-guard,trace
loftd --pty=raw,no-focus-input,trace
LOFTD_TERMINAL_TRACE=1 loftd --pty=normalize
```

To collect repeatable PTY benchmark artifacts, run the repo-local benchmark
script. It records synthetic PTY baselines, launches a finite live loftd command
with `LOFTD_ATTACH_PROFILE=1`, parses host attach summaries plus guest summaries
when visible, and writes machine-readable reports under
`.omx/benchmarks/loftd-pty/` by default:

```bash
scripts/loftd-pty-benchmark.sh --iterations 3
```

Use `--loftd <path>` or `LOFTD_BIN=/path/to/loftd` when testing a specific
binary; `--loftd-cargo-run` is available as an explicit opt-in for source-tree
runs. Repeat `--loftd-arg <arg>` for environment-specific launch flags, for
example `--loftd-arg --rootfs-backend --loftd-arg btrfs-snapshot`. The
generated `metrics.jsonl` contains per-run records, `summary.json` contains
aggregate synthetic timings plus parsed host/guest profile objects, and
`logs/` preserves raw captured output for failed live runs; the live PTY path
records the combined PTY stream in stdout and may leave stderr empty.
`--skip-live` is only for local synthetic smoke checks; PTY optimization
evidence should use the live run so a missing host profile fails visibly. The
default live run is the `live-loftd-shell` smoke/profile scenario. To add
interactive live PTY samples, pass `--live-iterations <n>` and optionally
`--live-warmup <n>`; this adds `live-loftd-redraw-typing` records where the host
drives stdin marker lines through the PTY while the guest emits redraw bursts
and distinct output markers. These samples use one persistent live loftd session
by default, so larger runs measure the interactive PTY hot path without
repeating VM/libkrun/guest startup for every sample. Each persistent record is
tagged with `measurement_model: "persistent-session"`,
`persistent_session: true`, a shared `persistent_session_id`, the child process
pid, sample ordinal/count, and `session_lifecycle_elapsed_us`. Hot-window
elapsed time, per-marker latency stats, read-gap stats, and bytes-drained
evidence remain under `profile`, with aggregate values and `measurement_models`
under `summary.json`'s `scenario_profiles.live-loftd-redraw-typing`. Pass
`--live-per-sample-vm` to request the legacy VM/process-per-sample model; those
records are tagged `measurement_model: "per-sample-vm"` and are useful for
startup+lifecycle diagnostics rather than persistent-session hot-path
comparison. For a higher-sample comparison, prefer n=100, for example:

```bash
scripts/loftd-pty-benchmark.sh \
  --loftd .omx/builds/loftd-main/bin/loftd \
  --guest-init .omx/builds/loftd-main/bin/loftd-guest-init \
  --iterations 3 \
  --warmup 1 \
  --live-iterations 100 \
  --live-warmup 5 \
  --timeout 240
```

The benchmark uses
`--mem 2` for the live run by default to avoid measuring huge-memory VM boot
delay instead of PTY latency; pass `--no-default-live-mem` to test loftd's
default memory behavior, or repeat `--loftd-arg --mem --loftd-arg <GiB>` to
choose another size. The live run uses a btrfs-backed state directory under
`/home/dev/.local/share/containers` when available, even if the parent shell has
a non-btrfs `XDG_STATE_HOME`; override that with `--state-home <path>`. When
`result/bin/loftd-guest-init` exists, the runner also passes it as the live
guest init by default so the host and guest benchmark artifacts match; use
`--guest-init <path>` or `--no-default-guest-init` to override that behavior.
The optimized live benchmark requires the host attach profile; the guest profile
is recorded when the guest/libkrun console is visible. For a strict guest-profile
diagnostic run, add `--loftd-arg --log-level --loftd-arg debug` and
`--require-guest-profile`, but do not treat that debug-logging run as the clean
performance baseline. Optional `--rmux` records a non-nested rmux attach-drain
comparison when an executable rmux is available: the runner creates an isolated
detached rmux session, attaches through a child PTY, drains a finite redraw
workload, and adds `optional-rmux` elapsed stats plus
`profiles.rmux_attach_drain` read-gap/byte metrics to `summary.json`. The rmux
comparison is still optional and threshold-free; absent or unusable rmux records
a structured skip/failure without editing `/mnt/rmux`. The `--tmux` hook still
records a structured skip until an isolated finite tmux comparison is added.

Loftd troubleshooting FAQ:

- When a task ends under memory pressure, the guest kernel's own account of the
  kill is kept in `guest-kernel-console.log` in the task state directory, and
  loftd reports it when the task ends, for example:

  ```text
  loftd: guest kernel OOM-killed python3.13 (pid 705), anon-rss 3895792 kB
  ```

  A managed task keeps its supervisor out of the guest OOM killer's reach
  (`oom_score_adj` of -1000) while the shell and its children stay killable, so
  a single runaway process is killed instead of ending the whole microVM. The
  guest console also captures a kernel panic, which loftd reports as
  `guest kernel found no killable task and panicked` when the OOM killer had no
  victim left. Without the console capture a guest death under memory pressure
  is indistinguishable from a task that finished normally.

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

- The guest has two descriptor ceilings, and loftd keeps them consistent: the
  per-process `RLIMIT_NOFILE` and the guest-kernel-wide
  `/proc/sys/fs/file-max`. The guest kernel derives `file-max` from guest RAM at
  boot, which at small `--mem` values (for example `--mem 4`) lands below the
  guest `RLIMIT_NOFILE` hard limit. Guest bootstrap raises `fs.file-max` to at
  least that hard limit so a process cannot fail with system-wide `ENFILE`
  before it reaches its own limit. A kernel value already above the hard limit
  is left alone, and `loftd --mem <GiB>` still raises the kernel-derived
  default. `loftd-guest-init fd-report` prints both ceilings
  (`process.N.soft_limit`, `process.N.hard_limit`, and `system_fds_max`).

Active task control is loftd-native and does not use host Podman as a runtime
backend:

```bash
loftd ps
loftd kill <task-id-or-handle-selector>
```

`loftd ps` scans loftd's app state and lists active task VM records across all
workspaces by default. The human-readable table includes a short handle, full task
id, status, helper PID/session identity, start timestamp, image, and workspace
slug. For a full task id like `loftd-4138-178109091122334455`, the handle is
`loftd-4138`, so `loftd kill loftd-4138` can target that task without typing the
opaque suffix. You can also use a displayed-handle prefix of at least two
characters, such as `loftd kill lo`, when that prefix uniquely matches one
visible handle. For handles shaped like `<name>-<number>`, `loftd kill` also
accepts `<name-prefix>-<handle-number-prefix>` when it uniquely matches a
displayed handle; for example, `loftd kill lo-18` can target
`loftd-1845-<opaque>` through its displayed handle `loftd-1845`. The
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
VM worker into that namespace. In the default passt mode, the helper creates an
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

The default network mode is libkrun virtio-net/passt: loftd starts a `passt`
unix-socket backend inside the same namespace, sets guest env `LOFTD_USE_PASST=1`,
and calls `krun_add_net_unixstream()` before `krun_start_enter()`. Passing `--tsi`
opts into libkrun's virtio-vsock/TSI proxy mode. In that mode loftd does not add a
libkrun network device, but the libkrun VMM still starts from the pasta-backed
namespace so the guest's Podman-like host aliases can reach the host at
`169.254.1.2`. Both modes always materialize `/etc/hosts` with:

```text
169.254.1.2    host.containers.internal host.docker.internal
```

Use repeatable `-p, --publish SPEC` to expose guest services on host ports.
In the default passt mode, unprefixed publish specs default to TCP; `tcp:` and
`udp:` select passt `-t` and `-u` forwarding respectively, and passt owns deeper
grammar validation for ranges, bind-address suffixes, interfaces, and exclusions:

```bash
./result/bin/loftd -p tcp:8080:80 -p udp:5353:5353 -- bash -lc 'echo ok'
```

Passing `--tsi` switches to TSI mode, where loftd supports only simple TCP
`HOST_PORT:GUEST_PORT` mappings through a two-hop path: `pasta` listens in the
host-facing helper namespace and forwards `HOST_PORT` into the VM worker's
private network namespace, while libkrun `krun_set_port_map()` maps guest
listens on `GUEST_PORT` to that same target-namespace `HOST_PORT`:

```bash
./result/bin/loftd --tsi -p 8080:80 -- bash -lc 'python3 -m http.server 80'
```

TSI publish specs intentionally reject UDP, host bind addresses, port ranges,
random host ports, `all`/`none`, and protocol selectors.
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
the built-in tool config/state, compiler-cache, and container-store mounts, and
duplicate guest targets are rejected. Loftd intentionally does not support Podman SELinux
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
bind-mounted view of the task rootfs with the workspace, tool state, Cargo,
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
./result/bin/loftd --tsi -- bash -lc 'getent hosts host.docker.internal && curl -fsS http://host.docker.internal:18080/'
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
crun `krun.nested_virt=1` flow used by the OCI/libkrun path. This exposes
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

Refresh pinned `dolthub/dolt` prebuilt release metadata (tag and per-system
asset hashes) in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-dolt-prebuilt.sh
```

Refresh pinned `gastownhall/beads` prebuilt release metadata (tag and
per-system asset hashes; release asset names embed the tag without its leading
`v`) in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-beads-prebuilt.sh
```

Refresh pinned `@pydantic/monty-linux-x64-gnu` worker metadata (version, tarball
asset name, and SRI hash) in `nix/pins.nix` from the npm registry:

```bash
nix develop --command ./scripts/update-monty-prebuilt.sh
```

Refresh pinned `zeroqn/libkrun` prebuilt release metadata in `nix/pins.nix`
from the newest matching `loftd-*` tag that contains both required Linux assets.
Root `.#libkrun` and every shared consumer (`.#crun`, `.#podman`, `.#loftd`, images, and
`.#loftd-prebuilt`) use the same pinned prebuilt libkrun
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

Refresh pinned `dirgeSandboxPrebuiltRelease` metadata in `nix/pins.nix` from the
newest `zeroqn/dirge` GitHub release containing the sandboxed dirge prebuilt
asset:

```bash
nix develop --command ./scripts/update-dirge-sandbox-prebuilt.sh
```

Refresh pinned `omp` prebuilt release metadata in `nix/pins.nix` from `can1357/oh-my-pi`:

```bash
nix develop --command ./scripts/update-omp-prebuilt.sh
```

Refresh pinned `herdrdev/herdr` prebuilt release metadata (tag and per-system
asset hashes) in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-herdr.sh
```

Refresh pinned `zvec-ai/zvec-grep` source and npm dependency metadata in
`nix/pins.nix` (the updater also rejects a release whose `bin.zg` no longer
points at `dist/cli/index.js`, which `nix/pkgs/zvec-grep.nix` installs):

```bash
nix develop --command ./scripts/update-zvec-grep.sh
```

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
