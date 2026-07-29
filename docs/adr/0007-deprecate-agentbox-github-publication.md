# Deprecate agentbox GitHub publication

Status: accepted

Supersedes: ADR 0006

GitHub Actions publishes and tests loftd artifacts only. Agentbox source code,
workspace membership, default workspace membership, and local Nix outputs remain
available, but the repository no longer publishes new agentbox images or release
binaries.

## Context

Loftd now owns the active runtime and publication path. Repository-level Nix and
workflow contracts had been attached to `agentbox-host`, so simply excluding the
agentbox crates from CI would also have removed loftd coverage. Continuing to
publish agentbox artifacts would additionally represent the deprecated product as
actively supported.

## Decision

Loftd and shared repository contracts live in a dedicated test-only workspace
crate selected explicitly by GitHub CI. Agentbox-only tests remain with the
agentbox crates and continue to run under ordinary local workspace tests.

The image workflows publish only `ghcr.io/<owner>/loftd`. The release workflow
publishes only the neutral loftd ELF and checksum assets. Agentbox-related flake
outputs remain unchanged for local use.

## Consequences

New agentbox images and release binaries are no longer produced by this
repository's GitHub workflows. Existing published artifacts are not removed.
Local users may still build agentbox from source or its retained Nix outputs, but
those paths are deprecated and are not part of the GitHub CI allowlist.
