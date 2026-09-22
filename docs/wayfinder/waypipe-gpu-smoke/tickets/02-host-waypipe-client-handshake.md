---
label: wayfinder:research
title: Host waypipe client and vsock handshake
status: open
blocked_by: []
claimed_by: null
---

## Question

What exactly must the host run so the guest's already-implemented waypipe server connects
through loftd's vsock connector?

Known starting facts: loftd exposes the path passed to `--waypipe=ABS_PATH` to the guest
via `krun_add_vsock_port2(ctx, guest_port, path, listen=false)` (so libkrun *connects to*
that path when the guest dials the port, meaning a host **listener** must already exist
there); the guest runs `waypipe [--no-gpu] --vsock --socket <PORT> --display
loftd-waypipe-0 server -- sleep infinity` with `--no-gpu` and the software ICD only when
`LOFTD_GPU_DRM` is unset.

Answer:

1. The exact host command that creates the listening socket the connector needs - is it
   `waypipe client --socket <abs path>` (unix mode), and what env does it need
   (`XDG_RUNTIME_DIR`, `WAYLAND_DISPLAY` pointing at the compositor)?
2. What the client logs on connect and on guest disconnect, and whether dmabuf/GPU
   transfer negotiation (`--no-gpu` absent) is visible in that log.
3. Failure modes the smoke should preflight: socket path missing, a stale socket file, a
   client started after loftd, and a wrong path. What does each look like from the host
   and from guest-init (which waits up to 10s for its own display socket)?
4. Whether the host client needs to be running before loftd starts, before the guest
   connects, or either.

## Deliverable

The exact host command, observed log lines for connect/disconnect, and the preflight
checks the smoke should perform.
