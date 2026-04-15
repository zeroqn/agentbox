# AGENTS.md

## Purpose

This repository contains `agentbox`, a small Rust CLI that launches an interactive
Podman container shell with the current working directory mounted at
`/workspace`.

It also supports an optional host-side `fuse-overlayfs` mount rooted under
`.agentbox/` and bind-mounts the merged result into the container at `/nix/store`.

## Repository Layout

- `src/main.rs`: CLI parsing, Podman argument construction, overlay mount setup,
  mount inspection, cleanup, and unit tests.
- `flake.nix`: development shell, Rust package, and container image definition.
- `README.md`: user-facing build, run, and overlay usage documentation.
- `Cargo.toml` / `Cargo.lock`: Rust package metadata and dependency lockfile.

## Working Style

- Keep changes narrow and consistent with the existing single-binary structure.
- Prefer extending the unit tests in `src/main.rs` when changing CLI behavior or
  mount argument construction.
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
nix develop --command cargo test
```

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
