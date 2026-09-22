---
label: wayfinder:research
title: Host waypipe client and vsock handshake
status: closed
blocked_by: []
claimed_by: bob (pi session 2026-09-22)
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

## Resolution

**The host runs `waypipe client` on the socket path loftd is given, and the transport
dials lazily - only when a guest app connects.** Verified with `loftd --waypipe=<path>`
plus weston from ticket 01; evidence in `/home/dev/loftd/disk/chromium-smoke/t02/ev/`.

1. **Host command** (the listener the vsock connector needs):

```sh
XDG_RUNTIME_DIR=<private dir> WAYLAND_DISPLAY=<compositor socket> \
  waypipe -d --socket <abs path> client
```

   It creates the listening unix socket at `<abs path>`; the same path is passed to
   `loftd --waypipe=<abs path>`. `waypipe -d ... client` also turns on the log lines the
   smoke can score.
2. **loftd preflights the path and fails before booting the VM** (both observed, exit 1,
   no guest started):
   - path missing: `loftd: waypipe socket does not exist: <path>: No such file or
     directory (os error 2)`
   - path exists but is not a socket: `loftd: waypipe transport is not a Unix socket:
     <path>` (the client also fails: `Failed to bind socket at <path>: EADDRINUSE`).
   So the smoke must start the listener **before** launching loftd, and can rely on
   these messages when it does not.
3. **The dial is lazy.** With the VM up and no guest app connecting, the host client logs
   nothing beyond `waypipe version` / `Starting client main process`, even though the
   guest's app-side socket exists (`srwxr-xr-x /run/user/1000/loftd-waypipe-0`). The first
   guest app connection produces `Connection received` followed by the handshake. A mere
   `socat` connect to the display socket already triggers it (then immediately closes:
   `Received Close message`). **So "the guest socket exists" is not evidence of transport;
   the smoke needs a real guest Wayland client.**
4. **Handshake log lines** (host client, `-d`), the smoke's transport evidence:

```text
Connection received
waypipe version: 0.11.0
Starting client connection process
have read initial bytes
Connection header: 0x00010a88
Connection remote version is 17, local is 17, using 17
Connected waypipe-server not receiving video
Connected waypipe-server may use dmabufs: true
Entered main loop
Processing request: wl_compositor#3.create_surface(wl_surface#15:new_id)
```

   `may use dmabufs: true` reflects the guest running **without** `--no-gpu`, i.e. because
   `LOFTD_GPU_DRM=1`.
5. **Guest-side facts**: `LOFTD_WAYPIPE_PORT=50427`, `WAYLAND_DISPLAY=loftd-waypipe-0`,
   `XDG_RUNTIME_DIR=/run/user/1000`, and process `waypipe --vsock --socket 50427 --display
   loftd-waypipe-0 server -- sleep infinity` - matching `guest_init/components/waypipe.rs`.
6. **A guest app already presents through it.** The image ships `rio`
   (`/nix/store/...-rio-headless-bin-0.4.12-d656326/bin/rio`); during the run the host
   `ss -x` showed two ESTABLISHED connections to the client socket, the client log showed
   rio's request stream (`wl_registry.bind`, `wl_compositor.create_surface`,
   `xdg_wm_base.get_xdg_surface`, `wl_shm.create_pool`,
   `zwp_linux_dmabuf_v1.get_default_feedback`), and the **screenshot taken by the host
   compositor while the run was live shows a painted window**: 118 distinct colours, dark
   window pixels (`#2e2c2b`, `#292726`) on weston's `#7c7572` background. The transport and
   presentation both work; Chromium is now the only untested client.
7. Consequence for the smoke design: score the transport on the *client log* plus the
   *host screenshot pixels*, never on the guest socket's existence, and run a real guest
   Wayland client (Chromium per ticket 03, `rio` as a cheap fallback/control).
