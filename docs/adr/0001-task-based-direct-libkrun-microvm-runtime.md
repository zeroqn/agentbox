# 0001. Task-based direct-libkrun microvm runtime

Date: 2026-05-26

## Status

Accepted

## Context

Agentbox currently has Podman-backed runtime paths, including an existing `libkrun` command that still launches through Podman/OCI runtime machinery. We want a new runtime shape that uses libkrun directly for the VM run path, while preserving agentbox's current task-oriented developer workflow.

The reference designs pull in opposite directions:

- `krunvm` has a useful OCI-image-to-libkrun shape, but exposes named VM lifecycle commands.
- `muvm` demonstrates direct libkrun control, but uses the host root filesystem rather than OCI-image-derived roots.
- Existing agentbox behavior is one-shot and task-based: launch a clean development environment for the current workspace.

## Decision

Agentbox will introduce `microvm` as an explicit experimental runtime mode.

`microvm` is task-based: each run creates a clean task root filesystem derived from an OCI-image cache, launches it through direct libkrun APIs, and tears down the per-task root filesystem after exit. It does not introduce a named VM lifecycle for v1.

`microvm` keeps developer-environment affordances:

- the current workspace is mounted at `/workspace`;
- guest init remains the in-guest bootstrap;
- the default experience is an interactive task shell;
- persistent cache disks include `/nix` and the container store;
- persistent cache disks are scoped per workspace;
- rootless guest container tooling remains available inside the guest.

The host run path avoids Podman, crun, and runc. Buildah is acceptable for image ingestion and cache preparation, but not for launching the task environment. Image caches are identified by resolved image digest, not mutable tags.

The storage backend supports a btrfs fast path when available and a portable fallback otherwise. Normal task launch should remain rootless from the user's perspective.

`microvm` will not become the default runtime until its developer-environment contracts are proven.

## Consequences

This preserves current agentbox task UX while creating a cleaner direct-libkrun runtime boundary.

The implementation needs new host-side ownership for direct libkrun context setup, rootfs cache materialization, storage backend selection, and cleanup. It can reuse existing guest init and existing workspace-scoped state concepts.

The design intentionally avoids a krunvm-style named VM lifecycle, so users who want persistent named VM instances are outside the initial scope.

## Alternatives considered

### Named VM lifecycle

Rejected for v1 because the desired agentbox workflow is a clean task environment per run, not long-lived named VM instances.

### Immediate default runtime replacement

Rejected because direct-libkrun `microvm` still needs validation across image ingestion, storage, workspace mounts, persistent cache disks, networking, TTY behavior, and cleanup.

### Ban Buildah entirely

Rejected because Buildah is acceptable for image ingestion/cache preparation. The critical boundary is the host run path, not every image-preparation helper.

### Host root filesystem like muvm

Rejected because the desired root filesystem source is an OCI image cache, not the host root filesystem.
