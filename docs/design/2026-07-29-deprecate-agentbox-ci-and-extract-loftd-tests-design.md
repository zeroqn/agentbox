# Deprecate agentbox CI and extract loftd repository tests

## Status

Approved.

## Context

The repository is moving toward deprecating the agentbox crates while retaining their source code for now. GitHub Actions currently treats agentbox as an active product: the generic Rust test job tests the whole workspace, image workflows publish both agentbox and loftd images, and the release workflow publishes both agentbox and loftd binaries.

A separate ownership issue exists in `crates/agentbox-host/src/tests.rs`. That module contains a large suite of repository-wide string-contract tests for Nix packages, image assembly, release workflows, and loftd packaging. Those tests are attached to the agentbox host crate even when the behavior under test is loftd-specific or shared repository infrastructure. If the agentbox crates are removed from CI without extracting these tests, loftd loses relevant coverage.

## Goals

- Preserve loftd and shared repository contract coverage without running agentbox crates in GitHub CI.
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
- Change loftd runtime, container, Podman, FUSE, or microVM behavior.

## Considered approaches

### Dedicated repository contract-test crate

Add a test-only workspace package dedicated to loftd and shared repository contracts.

Advantages:

- Keeps repository-wide Nix and workflow assertions out of both runtime product crates.
- Gives CI an explicit package to select.
- Allows agentbox-only tests to remain available locally without keeping agentbox in CI.
- Preserves the existing fast Rust `include_str!` contract-test style.

Disadvantage:

- Adds a small workspace crate whose only purpose is repository validation.

This is the selected approach.

### Integration tests under `crates/loftd/tests/`

This avoids a new package and would run automatically with `cargo test -p loftd`, but it makes the loftd runtime crate own repository-wide Nix and GitHub workflow contracts. That conflicts with the desired ownership boundary.

### Convert the contracts into Nix checks

This could place image and package invariants closer to their Nix implementation, but it substantially expands the scope and changes the testing mechanism. It is not necessary for the requested extraction.

## Architecture

### New test package

Add `crates/loftd-repository-tests` as a test-only workspace package.

The package will:

- have no runtime binary;
- have no dependency on `agentbox-host`, `agentbox-guest-init`, `loftd`, or `loftd-guest-init` production code;
- read repository files with `include_str!`;
- own helper functions used to inspect Nix lists, Nix top-level attributes, and shell heredocs;
- contain loftd-specific and shared repository contract tests that must continue running in CI.

The package will be added to both workspace `members` and `default-members`. Agentbox crates remain in both lists. Plain local workspace commands therefore continue to test agentbox, while GitHub CI uses an explicit package allowlist.

### Test classification

The existing tests in `crates/agentbox-host/src/tests.rs` will be classified rather than moved wholesale.

Move to the new package:

- loftd release binary and neutral prebuilt package contracts;
- loftd image publication and release workflow contracts;
- shared seccomp policy packaging used by loftd;
- loftd image configuration, wrappers, tooling, allocator, and Nix DB metadata contracts;
- shared image contracts that remain relevant to the loftd image;
- assertions that GitHub publishing workflows no longer contain agentbox publication wiring.

Keep in `agentbox-host`:

- agentbox binary wrapper and runtime-helper packaging contracts;
- agentbox image compatibility and guest-init contracts;
- agentbox-only container and local Nix output assertions.

Split mixed tests:

- create a loftd-only or shared assertion in the new package;
- retain only the agentbox-specific assertion in `agentbox-host`;
- avoid making agentbox behavior a required contract of the new package;
- allow the new package to assert the absence of agentbox publication wiring because that absence is part of the deprecation contract.

No loftd-owned repository tests need to be extracted from `agentbox-guest-init`; the inspected guest crate does not contain equivalent loftd or repository-wide fixtures.

## GitHub CI

### Rust test workflow

Replace the workspace-wide command in `.github/workflows/test.yml` with an explicit package allowlist containing:

- `loftd`;
- `loftd-guest-init`;
- `loftd-attach-protocol`;
- `loftd-exec-protocol`;
- `loftd-repository-tests`.

The allowlist intentionally omits `agentbox-host` and `agentbox-guest-init`. This prevents future agentbox compilation or test failures from blocking GitHub CI while keeping local workspace testing unchanged.

### Release image workflow

Update `.github/workflows/publish_image.yml` to publish only loftd.

- Remove the agentbox matrix row.
- Rename dual-product workflow text where appropriate.
- Preserve loftd `latest` or release-tag publication.
- Preserve the immutable `sha-<short-sha>` loftd tag.
- Preserve the loftd guest-init payload verification.

### Development image workflow

Update `.github/workflows/publish_dev_image.yml` to publish only loftd.

- Remove the agentbox matrix row.
- Preserve the mutable `dev` loftd tag.
- Preserve the immutable `sha-<short-sha>` loftd tag.
- Preserve the loftd guest-init payload verification.

### Release binary workflow

Update `.github/workflows/publish_release.yml` to publish only loftd.

- Stop building `.#agentbox-musl-ci-sccache`.
- Remove agentbox asset-name calculation.
- Remove agentbox binary copying and smoke checks.
- Remove agentbox checksum entries and uploads.
- Remove agentbox references from generated release notes.
- Preserve the raw neutral dynamic loftd ELF verification.
- Preserve rolling alpha, versioned, and immutable SHA release behavior for loftd.

The local agentbox-related flake outputs remain available and unchanged; GitHub workflows simply stop selecting them.

## Documentation

Update `README.md` where it describes published artifacts.

The documentation will state that:

- GitHub Actions publishes loftd images and loftd release binaries only;
- agentbox source and local Nix outputs remain available but are deprecated;
- `ghcr.io/<owner>/agentbox` and new agentbox release binaries are no longer produced by this repository's workflows.

Add a new ADR that supersedes `docs/adr/0006-separate-agentbox-and-loftd-image-publication.md`. The historical ADR remains unchanged as a record of the earlier decision. The new ADR records that agentbox publication has ended while local source and build outputs remain.

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
- focused tests for `agentbox-host` and `loftd-repository-tests` to verify the split preserves intended local coverage;
- the exact CI-equivalent explicit Cargo package test command;
- `nix develop --command cargo clippy --all-targets --all-features -- -D warnings`
- `nix develop --command cargo deny check`
- `nix develop --command cargo test`
- YAML syntax and targeted action inspection for all changed workflows;
- targeted Nix builds for the loftd release binary output and loftd container output selected by publishing.

Runtime, Podman, FUSE, and microVM smoke tests are not required because this change does not intentionally alter runtime behavior or image contents. The publishing workflow retains its loftd container payload verification.

## Success criteria

- No loftd or shared repository contract test depends on `agentbox-host` being selected in CI.
- GitHub Rust CI runs only loftd packages, loftd protocol packages, and the new repository contract-test package.
- GitHub publishing workflows produce no agentbox images or binary assets.
- Agentbox crates still compile and test under an ordinary local workspace test.
- Agentbox source and local Nix outputs remain present.
- README and ADR documentation accurately describe the new publication policy.
