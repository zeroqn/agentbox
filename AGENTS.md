# AGENTS.md

## Purpose

This repository contains `agentbox`, a small Rust CLI that launches an interactive
Podman container shell with the current working directory mounted at
`/workspace`.

It also supports an optional host-side `fuse-overlayfs` mount rooted under
`.agentbox/` and bind-mounts the merged result into the container at `/nix/store`.

## Repository Layout

- `crates/agentbox-host/`: host-side `agentbox` CLI, Podman/libkrun runtime
  orchestration, mount/state handling, and host-side unit tests.
- `crates/agentbox-guest-init/`: in-guest `agentbox-guest-init` bootstrap
  binary, root/user setup, guest Podman preparation, status files, and tests.
- `flake.nix`: development shell, Rust packages, and container image definition.
- `README.md`: user-facing build, run, and overlay usage documentation.
- `Cargo.toml` / `Cargo.lock`: Rust workspace metadata and dependency lockfile.

## Working Style

- Keep changes narrow and consistent with the host/guest crate ownership split.
- Write host-crate behavioral tests under `crates/agentbox-host/src/`. These
  tests exercise the host crate's own public or internal APIs (CLI, runtime,
  Podman orchestration, mount, state). Do not add loftd-specific tests or
  cross-cutting repository-invariant checks (documentation strings, ADR prose,
  Nix file content assertions) here.
- Write guest-crate behavioral tests under `crates/agentbox-guest-init/src/`.
  These tests exercise in-guest bootstrap, root/user setup, guest Podman prep,
  or status logic.
- Write loftd host-crate tests under `crates/loftd/src/`.
- Write loftd guest-crate tests under `crates/loftd-guest-init/src/`.
- Update `README.md` whenever user-visible behavior, requirements, or run
  commands change.
- Preserve any existing user changes in the worktree. Do not revert unrelated
  edits.

## Development Workflow

Use the Nix development shell so required tools are available:

```bash
nix develop
```

Common commands:

```bash
cargo build
cargo test
nix build .#agentbox
nix build .#container
```

To run the CLI from a built artifact:

```bash
AGENTBOX_IMAGE=localhost/agentbox:latest ./result/bin/agentbox
```

To exercise host overlay mode:

```bash
AGENTBOX_HOST_NIX_OVERLAY=1 ./result/bin/agentbox
```

## Validation

For code changes, prefer this validation sequence:

```bash
nix develop --command cargo fmt --check
nix develop --command cargo clippy --all-targets --all-features -- -D warnings
nix develop --command cargo deny check
nix develop --command cargo test
```

Before committing code changes, make sure formatting, Clippy, cargo-deny, and
tests pass.

If behavior touches container runtime or FUSE integration, also verify manually:

- container starts successfully with `podman`
- `/workspace` is mounted as expected
- overlay mode creates or reuses `.agentbox/nix-upper`, `.agentbox/nix-work`,
  and `.agentbox/nix-merged`
- overlay mount is cleaned up after shell exit

## Safety Notes

- Do not remove or reset `.agentbox/` contents unless explicitly requested.
- Avoid destructive git operations unless explicitly requested.
- Treat Podman, FUSE, and host `/nix/store` assumptions as environment-dependent
  and verify them when changing overlay behavior.

## Edit Gate

Before editing, creating, deleting, moving, formatting, or otherwise modifying
any file outside `.omx/`, the agent must first discuss the intended change with
bob and receive explicit approval.

The discussion should establish the intended outcome, scope, affected files or
areas, and validation approach before any non-`.omx/` file changes occur.

Files under `.omx/` may be created or updated for planning, state, notes, logs,
or workflow coordination without prior approval.

Read-only inspection, searching, builds, tests, and diagnostics may proceed
without approval as long as they do not modify files outside `.omx/`.

## Communication

- When a user question is needed, address the user as `bob`.
