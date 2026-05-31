# Loftd Buildah-backed btrfs task rootfs

Status: accepted

Loftd's default `btrfs-snapshot` task rootfs path uses Buildah storage as the durable OCI image source. For each task, loftd creates a temporary Buildah working container, mounts its rootfs inside one `buildah unshare` transaction, validates the `loftd-guest-init` contract, and snapshots that mounted rootfs directly into the loftd task state directory.

Loftd does not create a second loftd-owned extracted image-rootfs cache for the default btrfs path. Buildah remains responsible for OCI image identity, local image presence, pulling, and freshness.

## Context

The earlier loftd plan considered a digest-addressed loftd image rootfs cache copied from `buildah mount` output. That duplicates data already managed by Buildah's containers/storage graph and makes first-run setup depend on an O(n) rootfs copy such as `cp -a --reflink=auto`.

The preferred loftd default is the fast path: when Buildah uses btrfs-compatible storage and the loftd task state is snapshot-compatible with the mounted rootfs, a btrfs subvolume snapshot creates the task rootfs without a recursive copy. The task rootfs remains loftd-owned after the Buildah working container is unmounted and removed.

## Decision

For `btrfs-snapshot`:

- default image selection is local-first: inspect `localhost/loftd:latest` locally and use it with `--pull=never` when present;
- if the local image is missing, use `ghcr.io/zeroqn/loftd:latest` with `--pull=missing`;
- `--pull-latest` uses `ghcr.io/zeroqn/loftd:latest` with `--pull=always`;
- explicit `--image` uses the supplied reference with `--pull=missing`, including digest-pinned references;
- the Buildah transaction validates exactly one executable `loftd-guest-init` before snapshotting;
- snapshot and cleanup are strict btrfs operations; no copy/reflink fallback occurs in this default path.

Future boot code consumes a `TaskRootfsHandle` and should not know how Buildah resolved, mounted, or cleaned the image source.

## Consequences

The default path avoids a duplicated extracted rootfs cache and avoids O(n) rootfs copies when btrfs snapshot preconditions are met.

A machine whose Buildah storage or loftd state directory is not compatible with btrfs snapshots receives a clear failure rather than a silent slower fallback.

The explicit `fuse-overlay` backend remains the future portable fallback, but it is not implemented by this btrfs default slice.

Loftd has no ref-to-digest metadata file for this default path. Buildah storage is the authority for local image state and freshness.

## Considered options

- Duplicate Buildah rootfs into a loftd-owned digest cache: rejected because it duplicates storage and reintroduces recursive copy cost.
- Use `cp -a --reflink=auto` as fallback: rejected for the default path because it is still O(n) over rootfs metadata and can silently become a full copy.
- Keep a `reflink` backend in loftd v1: rejected by the task-rootfs backend policy.
- Mount the Buildah rootfs directly as the task rootfs: rejected because the task rootfs must survive Buildah unmount/removal and be writable as loftd-owned task state.
