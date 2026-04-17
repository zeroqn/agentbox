# agentbox

Minimal Rust CLI for launching a Podman container `fish` shell with a
`starship` prompt, Codex installed in the image, and the current directory
mounted at `/workspace`.

By default, `agentbox` runs in rootless sidecar mode. This avoids bulk
`/nix/store` copy by using host `fuse-overlayfs` (`lowerdir` from the selected
image's `/nix`) and a reusable sidecar `nix-daemon` socket.

The older seeded mode is still available by setting
`AGENTBOX_NIX_SIDECAR=0`.
Use `--disable-nix-sidecar` to run seeded mode for a single invocation.

## Development

```bash
nix develop
cargo build
```

The development shell includes Rust toolchain support for linting and formatting
via `cargo clippy` and `cargo fmt`/`rustfmt`.

`nix develop` now defaults to an interactive `fish` shell with `starship`
initialized. To stay in the parent shell (for example `bash`) instead, use:

```bash
AGENTBOX_DISABLE_AUTO_FISH=1 nix develop
```

## Build

```bash
nix build .#agentbox
nix build .#agentbox-prebuilt
nix build .#agentbox-musl
nix build .#container
```

`agentbox-prebuilt` installs a published release binary instead of compiling the
Rust source locally. It is currently pinned only for `x86_64-linux`; use
`.#agentbox` as the source-build fallback on other systems or after an older
retained prebuilt release has been pruned.

## Use from another flake

For a downstream NixOS or Home Manager flake that wants the prebuilt binary:

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

If you need a permanent fallback that does not depend on retained release
artifacts, use `agentbox.packages.${pkgs.system}.agentbox` instead.

## Publish

GitHub Actions publishes the container image to `ghcr.io/<repo-owner>/agentbox`
on pushes to `main` and on pushed git tags. Pull requests do not trigger the
publish workflow.

The published image also contains the static `agentbox` binary from
`.#agentbox-musl`, exposed on `PATH` inside the image. That makes the GHCR image
usable as a distribution source for the host CLI binary in addition to the
interactive shell environment.

The repo-built container image is emitted with a small, explicit layer budget
(`maxLayers = 7`) to keep local loads reasonable while still improving layer
reuse. The image now uses a custom layer pipeline that keeps the stable Rust
toolchain in an earlier reusable store layer, moves `bun`, `fzf`, `gh`,
`neovim`, and `starship` into a dedicated higher tooling layer, and pushes
`codex` plus `oh-my-codex` into the last store layer, so Codex and
shell-tooling updates do not force the earlier toolchain-heavy layers to
churn. The image is built
directly from Nix-provided contents instead of layering on top of an upstream
container base image, so the published archive no longer inherits the many
pre-existing layers from `ghcr.io/nixos/nix`. Because it no longer inherits the
upstream image's multi-user Nix setup, the image build now recreates the
`nixbld` group with its builder-user membership and the `nixbld<N>` builder
users required by `nix-daemon`.

Default branch pushes publish:

- `ghcr.io/<repo-owner>/agentbox:latest`
- `ghcr.io/<repo-owner>/agentbox:sha-<12-char-commit>`

Tag pushes publish:

- `ghcr.io/<repo-owner>/agentbox:<git-tag>`
- `ghcr.io/<repo-owner>/agentbox:sha-<12-char-commit>`

A separate GitHub Actions workflow also publishes prebuilt `musl` binaries to
GitHub Releases for pushes to `main`:

- `alpha`: rolling prerelease kept as the latest convenience download
- `sha-<12-char-commit>`: commit-addressed prerelease for that exact commit

Each release uploads `agentbox-<runner-arch>-unknown-linux-musl` plus a matching
`.sha256` file. To keep the release list manageable, the workflow retains only
the most recent 20 `sha-*` prereleases and deletes older ones.

`flake.nix` keeps one pinned prebuilt release tag plus its download hash. Update
that pin after a new release is published with:

```bash
nix develop --command ./scripts/update-agentbox-prebuilt.sh
```

By default the script picks the newest retained `sha-*` release, recomputes the
binary SRI hash, and rewrites the pinned prebuilt metadata in `flake.nix`. On a
fresh checkout of this branch, the pin may still point at the rolling `alpha`
bootstrap release until the first `sha-*` release has been published and the
update script has been run once.

## Run

Show the CLI help:

```bash
nix develop --command cargo run -- --help
```

Load the image into Podman, then run the CLI:

```bash
nix build .#container
podman load < result
nix build .#agentbox
./result/bin/agentbox
```

When no image is explicitly set, `agentbox` prefers `localhost/agentbox:latest`
and automatically falls back to `ghcr.io/zeroqn/agentbox:latest`.
Use `--pull-latest` to pull and run `ghcr.io/zeroqn/agentbox:latest` explicitly,
or use `--image` / `AGENTBOX_IMAGE=...` to force any specific image.

```bash
./result/bin/agentbox --pull-latest
```

Build a static `musl` binary:

```bash
nix build .#agentbox-musl
./result/bin/agentbox
```

Build the currently pinned published prebuilt binary:

```bash
nix build .#agentbox-prebuilt
./result/bin/agentbox
```

The `agentbox-musl` output is intended to produce a statically linked Linux
binary for the host architecture. This only affects the host-side CLI binary;
`agentbox` still requires a working `podman` installation and the configured
container image at runtime.

The container image also includes this static `agentbox` binary on `PATH`, so a
published GHCR image can be used to extract the CLI artifact without rebuilding
it locally.

The container image starts an interactive `fish` shell with `starship`
initialized from a bundled snippet that the entrypoint copies into the
runtime-writable `/home/dev/.config/fish/conf.d/agentbox-starship.fish` path at startup, includes the
`codex` CLI, `curl`, `file`, `jq`, `less`, `tar`, and `tmux` on `PATH`, includes Python 3 with
PyYAML available for imports, includes Node.js plus the pinned `oh-my-codex`
package on `PATH` as `omx`, and now also includes the stable nixpkgs Rust
toolchain directly in the image (`cargo`, `rustc`, `clippy`, `rustfmt`,
`rust-analyzer`, and Rust standard-library sources via `RUST_SRC_PATH`) without
requiring the optional host `/nix` overlay. The image runs as uid/gid `1000`
with home directory `/home/dev`.

`oh-my-codex` is packaged into the image through Nix rather than installed at
runtime with `npm install -g`, so the image stays reproducible. Its pinned
version, source hash, and npm dependency hash live in `flake.nix`.

To refresh that pin set when upstream publishes a new release:

```bash
nix develop --command ./scripts/update-oh-my-codex.sh
```

The update script queries the latest GitHub release, computes the new source
hash, derives the required `npmDepsHash`, and rewrites the pinned values in
`flake.nix`. Review the diff, then rebuild the image.

Each run also ensures the host `~/.codex` directory exists and bind-mounts it
into the container at `/home/dev/.codex` so Codex state persists across
sessions.
It also creates `.agentbox/cargo` inside the project and bind-mounts that
directory into the container at `/home/dev/.cargo`, so Cargo registries and
caches persist per project without seeding Cargo home contents from the image.

The interactive `podman run` uses `--userns=keep-id` so the `/workspace`
bind mount preserves the host ownership mapping instead of appearing as
`root:root` in the container.

The image leaves the interactive process uid/gid to Podman `--userns=keep-id`
instead of hardcoding `1000:1000` in the image config. Its entrypoint then uses
`nss_wrapper` to extend temporary passwd and group files at runtime so tools
inside the shell can resolve the mapped numeric user and group as `dev`, even
when the host gid is not `1000`.

The interactive container also mounts `/tmp` as `tmpfs` (`rw,exec,mode=1777`)
so temporary files are not backed by the container's overlay root filesystem.

If you want seeded mode instead of sidecar, run:

```bash
./result/bin/agentbox --disable-nix-sidecar
```

On the first run, `agentbox` copies `/nix/store` and `/nix/var/nix` from the
container image into `.agentbox/nix/` and creates a project-local
`.agentbox/nix/var/log/nix` directory for writable derivation logs. Later runs
bind-mount those seeded directories back into the container so `nix build` and
`nix develop` reuse the same Nix state for that project without falling back to
an image-owned `/nix/var/log/nix` path.

The dedicated seeding container runs as `root` so it can copy the image's
`/nix` contents into the bind-mounted project cache. The normal interactive
agentbox shell still runs as uid/gid `1000:1000`.

This mode creates and reuses:

```text
.agentbox/
  cargo/
  nix/
    .seeded
    store/
    var/
      log/
        nix/
      nix/
```

Requirements:

- Podman must be able to run the image and copy its `/nix` contents into the
  mounted `.agentbox/nix` directory on first use
- `.agentbox/nix` must have enough space for the seeded Nix store, derivation
  logs, and later build outputs

If `.agentbox/nix/store` or `.agentbox/nix/var/nix` contains partial state
without `.agentbox/nix/.seeded`, `agentbox` treats that as inconsistent and
refuses to seed automatically.

## Rootless sidecar mode (default; no `/nix/store` seed copy)

Sidecar mode is enabled by default:

```bash
./result/bin/agentbox
```

In this mode agentbox:

1. resolves image identity with `podman image inspect`
2. mounts the image root with `podman image mount` (falling back to
   `podman unshare podman image mount` when needed) and uses `<mount>/nix` as
   `fuse-overlayfs` lowerdir when present; if the mount itself already looks
   like a Nix root (for example it directly contains `store/`), agentbox uses
   `<mount>` as the lowerdir fallback
3. mounts a project-local merged tree at `.agentbox/nix-merged` with:
   - upperdir: `.agentbox/nix-upper`
   - workdir: `.agentbox/nix-work`
4. starts/reuses a deterministic sidecar container named like `agentbox-nix-sidecar-<repo>-<hash>` running `nix-daemon`
5. launches the interactive task container with `.agentbox/nix-merged:/nix:ro`
   and `NIX_REMOTE=unix:///nix/var/nix/daemon-socket/socket`

Sidecar state metadata is stored in `.agentbox/nix-sidecar.state`.

Requirements:

- `podman` available on host `PATH`
- `fuse-overlayfs` available on host `PATH`

Notes:

- `--disable-nix-sidecar` or `AGENTBOX_NIX_SIDECAR=0` switch to seeded mode.
- Sidecar reuse is gated by image ID + lowerdir + merged mount + socket
  connectability checks; mismatches recreate the sidecar stack.
- Task containers in sidecar mode are labeled with sidecar identity; when the
  last labeled task container exits, agentbox now removes the idle sidecar
  container automatically.
- When `podman image mount` is unavailable in rootless mode, agentbox resolves
  lowerdir and mounts `fuse-overlayfs` through `podman unshare`.
- If `.agentbox/nix-sidecar.state` is stale or malformed, agentbox auto-clears it
  and recreates the sidecar stack (no manual state-file deletion required).
- If sidecar startup times out, agentbox now captures recent sidecar logs and
  attempts automatic sidecar + merged-mount cleanup before returning an error;
  manual `.agentbox/nix-merged` deletion is only needed if cleanup explicitly
  reports failure. Timeout errors also include sidecar state and socket-probe
  diagnostics when available.
- Task containers mount `/nix` read-only in this mode; writes are intended to
  flow through `nix-daemon`.

If you have host state from an older buggy build under `.agentbox/store` or
`.agentbox/var/nix` instead of `.agentbox/nix/`, move or remove that stale state
before retrying `./result/bin/agentbox`.
