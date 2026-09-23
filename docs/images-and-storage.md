# Images, storage, and memory

## Image selection

Image selection is materialized through Buildah for the btrfs-snapshot path: with no
image option, loftd first inspects `localhost/loftd:latest` and uses it with
`--pull=never` when present, otherwise loftd uses `ghcr.io/zeroqn/loftd:latest`
with `--pull=missing`. The flake's canonical `.#container` output builds that
local `localhost/loftd:latest` image with `loftd-guest-init enter` as its guest
contract. `--pull-latest` refreshes the canonical image through Buildah before
cache lookup, and `--image` uses exactly the supplied image reference with
`--pull=missing`. `--image` and `--pull-latest` are mutually exclusive.

## Image cache management

Loftd also exposes a local image-cache management surface:

```bash
loftd images list
loftd images sync ghcr.io/example/loftd:dev
loftd images sync ba5a514
loftd images remove --dry-run feedfacecafe
loftd images remove feedfacecafe
loftd images remove ghcr.io/example/loftd:d
```

`loftd images list` is read-only and reports a Buildah-aligned table with
`REPOSITORY`, `TAG`, short `IMAGE ID`, short `DIGEST`, `CACHE`, `BUILDAH`, and
`PATH` columns. Cached rows remain digest-keyed internally, but the default view
omits the redundant digest key and shows about twelve digest/image-id characters
for copyable selectors. Buildah inventory rows that do not have a matching loftd
cache entry are included as `CACHE=uncached` and `BUILDAH=local-only`; old or
untagged local Buildah rows preserve Buildah's literal `<none>` repository/tag
display.

`loftd images sync <reference-or-selector>` preserves full image-reference sync
behavior and can also resolve a unique visible local selector, such as a
repository/tag prefix, digest prefix, or Buildah image-id prefix, before
materializing through Buildah. If no local visible row matches, the argument is
treated as the image reference to sync; ambiguous local selectors fail before
staging.

`loftd images remove <image-selector>` removes only a matching loftd cache entry.
It accepts exact full digests (`sha256:...`), exact digest keys
(`sha256-...`), and unique visible-row prefixes from `images list` such as
digest, repository/tag, selected reference, or Buildah image-id prefixes.
Ambiguous selectors are refused with candidate rows, and selectors that match
only `CACHE=uncached` Buildah rows are refused because there is no loftd cache
entry to delete. Removal remains cache-first: loftd attempts
`buildah rmi <selected-reference>` only when a fresh Buildah image inspect
proves the selected reference still resolves to the same digest recorded in the
cache metadata. Missing, digestless, ambiguous, or mismatched local Buildah
images are left in place and reported as skipped.

`loftd images remove --dry-run <image-selector>` resolves the same selector and
guard chain without mutating cache or local Buildah state. The preview reports
the exact loftd cache entry and the final local Buildah target that would be
removed after the existing fallback chain (selected reference, cached image ID,
then Buildah inventory reference). Unlike real remove, dry-run fails when that
local Buildah removal would be skipped for any reason.

## Task rootfs backend

Loftd uses **task rootfs backend** terminology for the host-side mechanism that
materializes the clean task root filesystem. The default backend is
`btrfs-snapshot`: loftd keeps a digest-keyed btrfs image-source snapshot cache
under its per-user image state directory and snapshots that cached source into a
fresh per-task rootfs on same-digest restarts. Cache misses still use one
`buildah unshare` transaction to create a temporary Buildah working container,
mount the selected image rootfs, validate exactly one executable
`loftd-guest-init`, snapshot the mounted rootfs into loftd task state, and
remove the Buildah working container; known-digest misses then snapshot that task
rootfs into the digest-keyed source cache and write cache metadata. Cache hits
may inspect/refresh image metadata but avoid the Buildah working-container
lifecycle (`buildah from`, `mount`, `umount`, and `rm`). Unknown-digest runs use
the direct Buildah materialization path and do not write cache entries. There is
no `auto` backend, no initial loftd `reflink` backend, and no copy/reflink
fallback for the default btrfs path; choose `fuse-overlay` explicitly when the
future portable overlay path is wanted.

## Host `/nix` overlay

For `/nix`, normal loftd launches now use a workspace-scoped host kernel
overlayfs rather than attaching `loftd-nix.raw`. The lowerdir is the selected
image cache rootfs under
`$STATE/loftd/microvm/images/btrfs-snapshots/<digest-key>/rootfs/nix`; the
upper, work, merged, and lease files live under the workspace slug state root at
`$STATE/loftd/<workspace-slug>/nix-overlay/`. Host-overlay launches run the
libkrun helper transaction through `buildah unshare`, so the VM worker mounts
and later unmounts the overlay in the same rootless Buildah namespace that can
see the selected image-cache lowerdir. The mount still happens immediately
before prepared-root grafting, and the merged view is bound to guest `/nix`.
Existing `loftd-nix.raw` files are not migrated or deleted automatically.
When a mutable image tag resolves to a new digest, the host-overlay lowerdir
follows the newly selected image cache entry while the workspace-scoped upper,
work, and merged directories are reused. This intentionally preserves packages
or files written into the overlay upperdir while exposing updated lower-image
store objects that are not shadowed by upperdir entries or overlay whiteouts.
It is not a Nix database merge or repair step: persistent Nix profiles, gcroots,
database rows, and whiteouts can still describe a mixed state and may require
manual cleanup or a workspace overlay reset if they become inconsistent.

- host-overlay `/nix` is signaled to the guest with `LOFTD_NIX_OVERLAY=1` and
  `LOFTD_NIX_HOST_OVERLAY=1`; no `/nix` disk id/label is emitted in this mode.
- host-overlay `/nix` requires `buildah` on `PATH`; permission-denied kernel
  overlay failures should be diagnosed from inside `buildah unshare`, because
  plain outer-namespace `mount -t overlay` does not have the required rootless
  idmap/storage context.
- Nested/rootless Podman storage uses the workspace-scoped
  `loftd-containers.raw` btrfs disk by default. The host exposes that disk as
  `LOFTD_CONTAINERS` / `loftd-containers` for guest rootless container storage,
  and guest Podman uses the `btrfs` storage driver.
- `loftd --container-store raw-disk` remains accepted as an explicit
  compatibility spelling for the only supported container-store backend.
  `--container-store bind` is not supported, and loftd does not migrate old
  host-directory container stores.

## Guest memory and zram swap

When `--mem` is omitted, loftd now sizes the direct-libkrun VM to 80% of host
memory rounded down to whole GiB, matching the libkrun VM memory policy. Pass
`loftd --mem <GiB>` to override that default. Guest bootstrap also sets
`SCCACHE_DIR=/home/dev/.cache/sccache`, backed by loftd's shared state
`sccache` bind mount.

Guest RAM is fixed for the life of the microVM, so the guest also gets zram
swap: during `enter`, before the shell or any background preparation starts,
`loftd-guest-init` sets the device capacity in `/sys/block/zram0/disksize`,
bounds what the device may spend in `/sys/block/zram0/mem_limit`, signs it with
`mkswap`, and activates it with `swapon -p 100`. The capacity equals guest RAM
and the memory budget is a quarter of it. Capacity counts uncompressed bytes
and zram holds a page compressed, so compressible content costs a fraction of
the space it occupies, while the budget stops pages that do not compress from
being parked at roughly 1:1. The pinned `libkrunfw` kernel is built with
`CONFIG_SWAP` and `CONFIG_ZRAM` (zstd default, lzo available). Swap makes cold
anonymous pages reclaimable, which turns memory pressure into slower progress
instead of a guest OOM kill; it does not add memory, and a kernel without zram
records `state=unavailable` in `/run/loftd/swap.status` rather than failing the
session.

## Launch config

Loftd config lives at:

```text
$XDG_CONFIG_HOME/loftd/loftd.toml
```

or, when `XDG_CONFIG_HOME` is unset:

```text
$HOME/.config/loftd/loftd.toml
```

Supported launch-planning keys are:

```toml
[state]
location = "/home/dev/loftd-state"

[task-rootfs]
backend = "btrfs-snapshot" # or "fuse-overlay"
```

`[state].location` changes the base loftd state location; loftd appends
`/loftd/<workspace-slug>`. `--rootfs-backend` overrides
`[task-rootfs].backend` for a single run.
