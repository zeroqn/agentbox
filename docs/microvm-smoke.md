# Microvm manual smoke checklist

`agentbox microvm` is still experimental. This checklist records what must be
proven in a real direct-libkrun environment before promoting the runtime beyond
the current hardening slices.

## Automated evidence before manual smoke

Run from the repository root:

```bash
nix develop --command cargo test -p agentbox-host 'runtime::microvm' -- --nocapture
nix develop --command cargo test -p agentbox-host cli::tests::cli_microvm_help_mentions_experimental -- --nocapture
nix develop --command cargo fmt --check
nix develop --command cargo clippy --all-targets --all-features -- -D warnings
nix develop --command cargo deny check
nix develop --command cargo test
git diff --check
```

These tests prove the host contracts that do not require a real VM: cache hits
do not invoke Buildah, Buildah cache-miss ingestion is exercised with a fake
command runner, rootfs copies preserve executable modes and symlinks,
storage/backend errors are classified, preserve-debug paths are reported, the
launch config round-trips, and the direct libkrun FFI call order avoids host
`podman run`, `crun`, and `runc`.

## Host prerequisites

- `/dev/kvm` is available to the user running `agentbox`.
- `libkrun.so` is loadable by the hidden microvm helper.
- `libkrunfw.so` is available as required by the installed libkrun build.
- `buildah` is available when testing cache misses or refreshes.
- `btrfs`, `mkfs.btrfs`, `blkid`, and `fuse-overlayfs` are available.
- The selected OCI image contains exactly one executable
  `/nix/store/*/bin/agentbox-guest-init`; cache-miss ingestion refuses images
  that do not satisfy this compatibility marker contract.

## Smoke steps

1. Build the host binary and guest init:

   ```bash
   nix build .#agentbox
   nix build .#container
   ```

2. Run a cache-hit microvm task:

   ```bash
   AGENTBOX_IMAGE=ghcr.io/example/agentbox@sha256:<cached-digest> \
     ./result/bin/agentbox microvm --storage auto
   ```

3. Run a cache-miss ingestion task with Buildah on `PATH`:

   ```bash
   AGENTBOX_IMAGE=ghcr.io/example/agentbox:<tag-not-yet-cached> \
     ./result/bin/agentbox microvm --storage auto --preserve-debug
   ```

   Expected host-side evidence:
   - the first run invokes Buildah to create a working container, inspect its
     resolved digest, mount it, copy the mounted rootfs into the digest-keyed
     microvm cache, validate `agentbox-guest-init`, write
     `agentbox-compatible`, then unmount/remove the Buildah container;
   - a second run with the same mutable tag resolves through local ref metadata
     and does not require Buildah;
   - digest-pinned mismatches and empty Buildah digest output fail before ref
     metadata is written.

4. Inside the guest, verify:
   - `/workspace` is mounted from the host workspace.
   - `/nix` uses the persistent microvm `/nix` disk.
   - Rootless container storage uses the persistent microvm container-store
     disk.
   - Outbound networking works through libkrun's no-passt/TSI default path.
   - Inbound port publishing is unavailable by design.
   - Terminal input/output is usable through the default virtio-console path.
   - Terminal resize behavior is recorded as pass/fail for the current host.

5. Exit and verify cleanup:
   - Without `--preserve-debug`, the task rootfs is removed.
   - With `--preserve-debug`, the task rootfs and task state directory remain
     and the failure/success diagnostics point to them.
   - `<state-root>/microvm-nix.raw` and
     `<state-root>/microvm-containers.raw` are reused across runs.

## Current manual result

As of 2026-05-27, real direct-libkrun smoke and real host Buildah cache-miss
smoke have not been run in this workspace. The automated contracts above are
the current evidence. Do not claim real VM boot, outbound networking, terminal
resize, or real Buildah unpack success until this file is updated with host
details, commands, and observed output.
