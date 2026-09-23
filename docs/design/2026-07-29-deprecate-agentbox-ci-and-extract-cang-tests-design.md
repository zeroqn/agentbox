# Deprecate agentbox CI and extract cang repository tests

## Status

Approved.

## Context

The repository is moving toward deprecating the agentbox crates while retaining their source code for now. GitHub Actions currently treats agentbox as an active product: the generic Rust test job tests the whole workspace, image workflows publish both agentbox and cang images, and the release workflow publishes both agentbox and cang binaries.

A separate ownership issue exists in `crates/agentbox-host/src/tests.rs`. That module contains a large suite of repository-wide string-contract tests for Nix packages, image assembly, release workflows, and cang packaging. Those tests are attached to the agentbox host crate even when the behavior under test is cang-specific or shared repository infrastructure. If the agentbox crates are removed from CI without extracting these tests, cang loses relevant coverage.

## Goals

- Preserve cang and shared repository contract coverage without running agentbox crates in GitHub CI.
- Stop publishing agentbox images and binaries from GitHub Actions.
- Keep agentbox production source, workspace membership, default workspace membership, and local Nix outputs intact.
- Give repository-level contract tests an owner independent of either product crate.
- Document that agentbox is retained locally but deprecated and no longer published by GitHub Actions.

## Non-goals

- Remove or refactor agentbox production code.
- Remove agentbox crates from workspace `members` or `default-members`.
- Remove local Nix outputs such as `agentbox`, `agentbox-container`, or `agentbox-musl`.
- Refactor shared image or Nix architecture solely because it continues to support local agentbox builds.
- Add a replacement migration or publication mechanism for existing agentbox users.
- Change cang runtime, container, Podman, FUSE, or microVM behavior.

## Considered approaches

### Dedicated repository contract-test crate

Add a test-only workspace package dedicated to cang and shared repository contracts.

Advantages:

- Keeps repository-wide Nix and workflow assertions out of both runtime product crates.
- Gives CI an explicit package to select.
- Allows agentbox-only tests to remain available locally without keeping agentbox in CI.
- Preserves the existing fast Rust `include_str!` contract-test style.

Disadvantage:

- Adds a small workspace crate whose only purpose is repository validation.

This is the selected approach.

### Integration tests under `crates/cang/tests/`

This avoids a new package and would run automatically with `cargo test -p cang`, but it makes the cang runtime crate own repository-wide Nix and GitHub workflow contracts. That conflicts with the desired ownership boundary.

### Convert the contracts into Nix checks

This could place image and package invariants closer to their Nix implementation, but it substantially expands the scope and changes the testing mechanism. It is not necessary for the requested extraction.

## Architecture

### New test package

Add `crates/cang-repository-tests` as a test-only workspace package.

The package will:

- have no runtime binary;
- have no dependency on `agentbox-host`, `agentbox-guest-init`, `cang`, or `cang-guest-init` production code;
- read repository files with `include_str!`;
- own helper functions used to inspect Nix lists, Nix top-level attributes, and shell heredocs;
- contain cang-specific and shared repository contract tests that must continue running in CI.

The package will be added to both workspace `members` and `default-members`. Agentbox crates remain in both lists. Plain local workspace commands therefore continue to test agentbox, while GitHub CI uses an explicit package allowlist.

### Test classification

The existing tests in `crates/agentbox-host/src/tests.rs` will be classified rather than moved wholesale.

Move to the new package:

- cang release binary and neutral prebuilt package contracts;
- cang image publication and release workflow contracts;
- shared seccomp policy packaging used by cang;
- cang image configuration, wrappers, tooling, allocator, and Nix DB metadata contracts;
- shared image contracts that remain relevant to the cang image;
- assertions that GitHub publishing workflows no longer contain agentbox publication wiring.

Keep in `agentbox-host`:

- agentbox binary wrapper and runtime-helper packaging contracts;
- agentbox image compatibility and guest-init contracts;
- agentbox-only container and local Nix output assertions.

Split mixed tests:

- create a cang-only or shared assertion in the new package;
- retain only the agentbox-specific assertion in `agentbox-host`;
- avoid making agentbox behavior a required contract of the new package;
- allow the new package to assert the absence of agentbox publication wiring because that absence is part of the deprecation contract.

No cang-owned repository tests need to be extracted from `agentbox-guest-init`; the inspected guest crate does not contain equivalent cang or repository-wide fixtures.

## GitHub CI

### Rust test workflow

Replace the workspace-wide command in `.github/workflows/test.yml` with an explicit package allowlist containing:

- `cang`;
- `cang-guest-init`;
- `cang-attach-protocol`;
- `cang-exec-protocol`;
- `cang-repository-tests`.

The allowlist intentionally omits `agentbox-host` and `agentbox-guest-init`. This prevents future agentbox compilation or test failures from blocking GitHub CI while keeping local workspace testing unchanged.

### Release image workflow

Update `.github/workflows/publish_image.yml` to publish only cang.

- Remove the agentbox matrix row.
- Rename dual-product workflow text where appropriate.
- Preserve cang `latest` or release-tag publication.
- Preserve the immutable `sha-<short-sha>` cang tag.
- Preserve the cang guest-init payload verification.

### Development image workflow

Update `.github/workflows/publish_dev_image.yml` to publish only cang.

- Remove the agentbox matrix row.
- Preserve the mutable `dev` cang tag.
- Preserve the immutable `sha-<short-sha>` cang tag.
- Preserve the cang guest-init payload verification.

### Release binary workflow

Update `.github/workflows/publish_release.yml` to publish only cang.

- Stop building `.#agentbox-musl-ci-sccache`.
- Remove agentbox asset-name calculation.
- Remove agentbox binary copying and smoke checks.
- Remove agentbox checksum entries and uploads.
- Remove agentbox references from generated release notes.
- Preserve the raw neutral dynamic cang ELF verification.
- Preserve rolling alpha, versioned, and immutable SHA release behavior for cang.

The local agentbox-related flake outputs remain available and unchanged; GitHub workflows simply stop selecting them.

## Documentation

Update `README.md` where it describes published artifacts.

The documentation will state that:

- GitHub Actions publishes cang images and cang release binaries only;
- agentbox source and local Nix outputs remain available but are deprecated;
- `ghcr.io/<owner>/agentbox` and new agentbox release binaries are no longer produced by this repository's workflows.

Add a new ADR that supersedes `docs/adr/0006-separate-agentbox-and-cang-image-publication.md`. The historical ADR remains unchanged as a record of the earlier decision. The new ADR records that agentbox publication has ended while local source and build outputs remain.

## Error handling and failure behavior

The repository contract tests remain ordinary Rust tests.

- Missing required wiring fails with the missing contract string in the assertion message.
- Forbidden agentbox publication wiring fails with the unexpected string in the assertion message.
- An invalid or missing package in the CI allowlist causes Cargo to fail rather than silently skipping tests.
- Existing GitHub Actions commands retain normal nonzero-exit failure behavior.

No fallback to agentbox artifacts is introduced.

## Validation

Run the following checks after implementation:

- `nix develop --command cargo fmt --check`
- focused tests for `agentbox-host` and `cang-repository-tests` to verify the split preserves intended local coverage;
- the exact CI-equivalent explicit Cargo package test command;
- `nix develop --command cargo clippy --all-targets --all-features -- -D warnings`
- `nix develop --command cargo deny check`
- `nix develop --command cargo test`
- YAML syntax and targeted action inspection for all changed workflows;
- targeted Nix builds for the cang release binary output and cang container output selected by publishing.

Runtime, Podman, FUSE, and microVM smoke tests are not required because this change does not intentionally alter runtime behavior or image contents. The publishing workflow retains its cang container payload verification.

## Success criteria

- No cang or shared repository contract test depends on `agentbox-host` being selected in CI.
- GitHub Rust CI runs only cang packages, cang protocol packages, and the new repository contract-test package.
- GitHub publishing workflows produce no agentbox images or binary assets.
- Agentbox crates still compile and test under an ordinary local workspace test.
- Agentbox source and local Nix outputs remain present.
- README and ADR documentation accurately describe the new publication policy.
