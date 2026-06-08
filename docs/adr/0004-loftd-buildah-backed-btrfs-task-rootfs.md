# Loftd digest-keyed btrfs image-source cache

Status: accepted (revises the original Buildah-per-task btrfs decision)

Loftd's default `btrfs-snapshot` task rootfs path uses Buildah as the authority
for OCI image resolution, local image presence, pulling, and freshness. For
same-digest restarts, loftd also maintains a digest-keyed btrfs image-source
snapshot cache under its per-user image state directory so task rootfs
materialization can snapshot a cached source directly into fresh task state.

This revises the earlier ADR 0004 position that rejected a loftd-owned extracted
image-rootfs cache for the btrfs path. The revised cache is btrfs-only,
digest-keyed, and snapshot-based; it is not a mutable-tag cache and it does not
introduce recursive copy or reflink fallback behavior.

## Context

The earlier loftd plan preferred snapshotting directly from a Buildah-mounted
rootfs for every task to avoid duplicating Buildah's containers/storage data and
to avoid O(n) copies such as `cp -a --reflink=auto`. Profiling later showed that
same-digest restarts still paid repeated Buildah working-container lifecycle cost
(`buildah from`, `buildah mount`, `buildah umount`, `buildah rm`) during
`task_rootfs_materialization`.

The desired optimization target is same-digest restarts, not first-run latency.
A btrfs source snapshot cached by resolved digest preserves fresh per-task rootfs
semantics while avoiding repeated Buildah working-container mount lifecycle on
cache hits.

## Decision

For `btrfs-snapshot`:

- default image selection remains local-first: inspect `localhost/loftd:latest`
  locally and use it with `--pull=never` when present;
- if the local image is missing, use `ghcr.io/zeroqn/loftd:latest` with
  `--pull=missing`;
- `--pull-latest` refreshes `ghcr.io/zeroqn/loftd:latest` through Buildah before
  cache lookup;
- explicit `--image` uses the supplied reference with `--pull=missing`, including
  digest-pinned references;
- cache entries are keyed only by known resolved digest, using filesystem-safe
  digest keys under loftd's per-user image state directory;
- cache-hit materialization snapshots the cached btrfs source rootfs into a fresh
  per-task rootfs and must not create, mount, unmount, or remove a Buildah
  working container;
- cache misses use the existing Buildah materializer transaction to validate
  exactly one executable `loftd-guest-init` and materialize the task rootfs;
- known-digest misses populate or rebuild the digest-keyed btrfs source cache
  from the materialized task rootfs and write metadata only after the cache
  source snapshot succeeds;
- unknown-digest runs use the direct Buildah materialization path and write no
  cache entry;
- snapshot and cleanup remain strict btrfs operations; no copy/reflink fallback
  occurs in this default path.

The boot pipeline consumes a `TaskRootfsHandle` and profile metadata. It should
not own cache layout, Buildah lifecycle details, or cache metadata semantics.

## Consequences

Same-digest btrfs-snapshot restarts avoid the repeated Buildah working-container
lifecycle and should reduce `task_rootfs_materialization` when the cache entry is
valid.

First runs and new digest runs still use Buildah and may do one extra btrfs
snapshot to populate the source cache. Cache garbage collection is deferred;
entries may accumulate under loftd image state until a future cache-management
slice.

A machine whose Buildah storage, loftd task state, or loftd image cache directory
is not compatible with btrfs snapshots receives a clear failure rather than a
silent slower fallback.

The explicit `fuse-overlay` backend remains the future portable fallback, but it
is not implemented by this btrfs default slice.

Profile output reports cache status with `task_rootfs_cache_status` values
`hit`, `miss-populated`, `miss-rebuilt`, or `direct-uncached`, plus digest key,
optional cache path, and uncached reason metadata where applicable.

## Considered options

- Keep direct Buildah-per-task snapshotting only: rejected because same-digest
  restarts keep paying Buildah working-container lifecycle cost.
- Use a mutable tag cache key: rejected because image source identity must be a
  resolved digest.
- Use `cp -a --reflink=auto` as fallback: rejected for the default path because
  it is still O(n) over rootfs metadata and can silently become a full copy.
- Keep a `reflink` backend in loftd v1: rejected by the task-rootfs backend
  policy.
- Mount the cached source directly as the task rootfs: rejected because each task
  must receive a fresh writable task rootfs that is cleaned independently.
