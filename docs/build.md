# Build outputs and Nix DB diagnostics

## Build outputs

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

## Nix store / DB diagnostics

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
