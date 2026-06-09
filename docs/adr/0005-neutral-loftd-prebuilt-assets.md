# Neutral loftd prebuilt release assets

Status: accepted

Loftd release assets consumed by `.#loftd-prebuilt` are neutral dynamic Linux
ELF payloads named `loftd-<arch>-unknown-linux-gnu`. They are packaging inputs,
not standalone portable executables and not flake-locked Nix outputs.

The GitHub release workflow strips release-builder `/nix/store/<hash>-...`
interpreter and RPATH references before upload. The Nix package then uses
`autoPatchelfHook` to bind ordinary ELF dependencies to the consuming flake and
provides package-relative runtime tools plus `libkrun`/`libkrunfw` paths without
wrapping `bin/loftd`.

## Context

The previous `loftd-<arch>-linux-flake-locked` asset embedded glibc and GCC
runtime paths from the release builder's `/nix/store`. `pkgs.fetchurl` consumes
that asset as fixed-output bytes, so Nix rejects the fetch for referring to other
store paths before package fixup can run.

Using `unsafeDiscardReferences` would hide the fixed-output reference check while
preserving the misleading release contract. The better long-term boundary is a
neutral upstream asset plus Nix-side patching in the package that consumes it.

## Decision

- Publish `loftd-<arch>-unknown-linux-gnu` assets for loftd prebuilts.
- Reject legacy `loftd-<arch>-linux-flake-locked` pins before fetching them.
- Fail release and updater flows when a loftd asset contains concrete
  `/nix/store/<hash>-...` references.
- Use `autoPatchelfHook` in `.#loftd-prebuilt` for ordinary ELF runtime
  dependencies.
- Keep `libkrun` and `libkrunfw` runtime-loaded through package-relative
  lookup semantics instead of making them required ELF `NEEDED` dependencies.

## Consequences

Existing legacy pins fail early until a new neutral `sha-*` release asset is
published and pinned.

The raw GitHub asset is honest about its role: it is a neutral dynamic Linux ELF
for packaging. Ordinary users should prefer `nix build .#loftd`,
`nix build .#loftd-prebuilt`, or the published `ghcr.io/<repo-owner>/loftd`
image.

## Considered options

- Keep flake-locked release assets: rejected because fixed-output fetches still
  see stale `/nix/store/<hash>-...` references and the public asset name
  promises the wrong contract.
- Use `unsafeDiscardReferences`: rejected because it bypasses the symptom instead
  of fixing the asset boundary.
- Promise a standalone portable dynamic `loftd`: rejected because host loftd is
  dynamically linked and also expects package-provided runtime tools and libkrun
  libraries.
