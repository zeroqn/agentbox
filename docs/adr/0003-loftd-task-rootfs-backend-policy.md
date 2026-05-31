# Loftd task rootfs backend policy

Status: accepted

Loftd selects the host-side task rootfs backend explicitly rather than probing for an automatic fallback.

The default loftd task rootfs backend is `btrfs-snapshot`. If btrfs snapshot storage cannot be used, loftd should fail with a clear diagnostic unless the user explicitly selects another backend.

Loftd's initial fallback backend is `fuse-overlay`, selected deliberately through loftd configuration or a CLI override. The persistent config shape is:

```toml
[task-rootfs]
backend = "btrfs-snapshot"
```

The CLI override is `--rootfs-backend`.

Loftd does not include an `auto` backend policy. Loftd also does not include `reflink` in its initial task rootfs backend set.

## Context

Agentbox microvm previously used broader storage-backend language and included automatic behavior that could try btrfs first and then fall back to fuse-overlay. That portability is useful, but it requires backend probing and can make performance and behavior differ silently across machines.

Loftd is being extracted as a cleaner direct-libkrun runtime owner. Its initial public CLI/config shape should avoid ambiguous storage language and avoid conflating task rootfs materialization with container/Buildah storage configuration.

## Consequences

The loftd default path is opinionated and predictable: users get btrfs snapshot semantics or a clear error.

Users on non-btrfs hosts must opt into `fuse-overlay` explicitly rather than receiving it through silent fallback.

The implementation does not need an automatic backend probing feature for the initial loftd launch-planning slice.

The public config and CLI language use “task rootfs backend” rather than generic “storage backend”.

## Considered options

- Keep `auto`: rejected because it requires probing and can silently change behavior across hosts.
- Use `[storage].backend`: rejected because “storage” conflicts with container/Buildah storage language.
- Include `reflink`: rejected for the initial loftd backend set because it adds a third backend surface without serving the default btrfs path or the explicit portable fallback.
- Remove fuse-overlay entirely: rejected because users still need an explicit non-btrfs fallback path.
