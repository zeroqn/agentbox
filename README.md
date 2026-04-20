# agentbox

`agentbox` is a small Rust CLI that starts an interactive Podman container shell
for your current project.

It mounts the current directory at `/workspace`, persists Codex state from the
host, and supports two Nix runtime modes:

- **Rootless sidecar mode (default):** uses host `fuse-overlayfs` + a reusable
  `nix-daemon` sidecar (no `/nix/store` seed copy).
- **Seeded mode (fallback):** copies `/nix` into project state on first run.

---

## Prerequisites

- Linux
- `podman`
- `nix` (for building via flake)
- `fuse-overlayfs` (required for default sidecar mode)

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

---

## Build

```bash
nix build .#agentbox
nix build .#agentbox-prebuilt
nix build .#agentbox-musl
nix build .#container
```

### Build outputs

- `.#agentbox`: compile from source.
- `.#agentbox-prebuilt`: install pinned published binary (currently pinned for
  `x86_64-linux`; use `.#agentbox` elsewhere).
- `.#agentbox-musl`: static host binary.
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
4. Starts/reuses a deterministic `nix-daemon` sidecar.
5. Starts the interactive container with read-only `/nix` + daemon socket.

Sidecar metadata is saved at:

```text
<state-root>/nix-sidecar.state
```

Disable sidecar mode for one run:

```bash
./result/bin/agentbox --disable-nix-sidecar
```

Or globally via env:

```bash
AGENTBOX_NIX_SIDECAR=0 ./result/bin/agentbox
```

---

### 2) Seeded mode (legacy fallback)

First run copies image `/nix/store` and `/nix/var/nix` into project state,
then reuses that data across runs.

Use seeded mode:

```bash
./result/bin/agentbox --disable-nix-sidecar
# or
AGENTBOX_NIX_SIDECAR=0 ./result/bin/agentbox
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

---

## Container environment summary

The container provides:

- interactive `fish` + `starship`
- Codex CLI + `oh-my-codex` (`omx`)
- Python 3 (`PyYAML`), Node.js
- Rust toolchain (`cargo`, `rustc`, `clippy`, `rustfmt`, `rust-analyzer`)
- `gcc`, `musl`, `clang`
- `LIBCLANG_PATH` preset to the bundled Nix `libclang` library directory
- common tools (`curl`, `jq`, `tmux`, etc.)

It runs with `--userns=keep-id` so `/workspace` ownership matches host mapping.

---

## Publishing

### Container image (GitHub Actions)

On push to `main` and tag pushes, CI publishes to:

- `ghcr.io/<repo-owner>/agentbox:latest` (main only)
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

Refresh pinned `oh-my-codex` version/hashes in `nix/pins.nix`:

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
