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
- Prefer extending host tests under `crates/agentbox-host/src/` when changing
  CLI, runtime, Podman, mount, or state behavior.
- Prefer extending guest tests under `crates/agentbox-guest-init/src/` when
  changing in-guest bootstrap, root/user setup, guest Podman prep, or status
  behavior.
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
AGENTBOX_IMAGE=localhost/loftd:latest ./result/bin/agentbox
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

- Before changing repo-tracked files, create a plan file in `.omx/plans/`
  named `YYYY-MM-DD-HHMM-short-slug.md`, present the plan to the user, and ask
  for confirmation.
- Do not change code until the relevant plan file exists and has been presented
  to the user.
- Implement changes according to the approved plan file. If the implementation
  needs to diverge materially, update the plan file first and present the
  revised plan before continuing.
- Plan generation may be skipped only when bob explicitly approves that the
  change is small. Agents must not make that determination unilaterally.
- When a user question is needed, address the user as `bob`.
