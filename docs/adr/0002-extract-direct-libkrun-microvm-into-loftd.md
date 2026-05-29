# Extract direct-libkrun microvm into loftd and loftd-guest-init

Status: accepted

Agentbox already contains an experimental direct-libkrun `microvm` path, but the long-term owner will be separate workspace crates named `loftd` and `loftd-guest-init` so the microvm runtime can have a clean identity, state root, image contract, and host dependency boundary without inheriting agentbox's Podman-backed runtime selection.

`loftd` will be a single-runtime CLI that launches microvm tasks directly, owns loftd-namespaced runtime state with an agentbox-like config override, defaults to loftd-compatible images, uses `LOFTD_*` runtime contracts, and is packaged as a normal dynamically linked binary. It must not depend on host Podman; Buildah remains allowed for image ingestion and explicit image refresh, including `--pull-latest`, and may reuse the user's normal containers configuration such as `~/.config/containers`.

`loftd-guest-init` will preserve the guest bootstrap role and rootless Podman preparation inside the VM, but it will use canonical loftd naming and will not accept legacy `AGENTBOX_*` aliases. Existing `agentbox microvm` code stays in place until `loftd` and `loftd-guest-init` are complete; the first extraction slice creates compiling structure-preserving skeleton crates rather than bulk-copying a renamed runtime.

## Considered options

- Keep direct microvm as `agentbox microvm`: rejected because loftd needs a clean host dependency boundary, image identity, state root, and single-runtime CLI.
- Remove `agentbox microvm` immediately: rejected because the existing path remains useful until loftd is complete.
- Ban all host container tooling: rejected because Buildah is still the image-ingestion and image-refresh boundary, distinct from Podman-backed runtime launch.
- Accept `AGENTBOX_*` aliases in loftd-guest-init: rejected because the new guest contract should be canonical from the start.
- Use the shorter `loftd-init` name: rejected because `loftd-guest-init` mirrors `agentbox-guest-init` and makes the guest boundary explicit.
