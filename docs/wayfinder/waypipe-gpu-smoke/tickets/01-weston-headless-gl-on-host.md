---
label: wayfinder:research
title: weston headless + GL on this host
status: closed
blocked_by: []
claimed_by: bob (pi session 2026-09-22)
---

## Question

Can weston 15.0.1 (`/nix/store/2rw2abdpndf6wijhx0cc4dx8gyg2jwfl-weston-15.0.1`, which
ships `lib/libweston-15/headless-backend.so`, `gl-renderer.so` and
`bin/weston-screenshooter`) serve a Wayland socket on this host with
`--backend=headless --renderer=gl`?

Specifically:

1. Does the GL renderer initialise with no seat and no VT, via surfaceless EGL or GBM on
   `/dev/dri/renderD128` (host also has `card1`)? Report `WESTON_*`/EGL env needed and
   the exact failure (exit code + message) when GL cannot init, since the smoke must
   hard-fail on that.
2. Which flags/env are required to run it: `XDG_RUNTIME_DIR`, `-S/--socket` name,
   `--width/--height`, anything else. Does it need `--shell=kiosk` for a single app?
3. Does `weston-screenshooter` capture the composited output of a headless weston, where
   does it write the PNG, what does it do with no client attached, and can the smoke rely
   on it as the host-side frame evidence?
4. Starting weston headless must not disturb the host (no compositor or X session is
   running; sessions are ttys on pts/0-1). Confirm it takes no DRM master and leaves the
   host session alone.

## Deliverable

A transcript with the exact command lines plus observed output, and a one-line verdict
per numbered item.

## Resolution

**Yes — weston 15.0.1 serves a headless compositor on the host GPU, and it can
screenshot itself.** Verified with the exact commands below; evidence kept in
`/home/dev/loftd/disk/chromium-smoke/weston-test2/`.

Working invocation:

```sh
export XDG_RUNTIME_DIR=<private dir, mode 700>
weston --backend=headless --renderer=gl --debug \
       --width=640 --height=480 --socket=wayland-wp --no-config --log=<file>
```

1. **GL initialises with no seat and no VT.** Log: `Using rendering device:
   /dev/dri/renderD128`, `EGL version: 1.5`, `GL renderer: AMD Radeon RX 7600M XT
   (radeonsi, navi33, ACO, DRM 3.64, ...)`, `Using GL renderer`. It opens only
   `/dev/dri/renderD128` (~6 fds, no `card1`), so it takes no DRM master and the
   host's tty sessions (loginctl sessions 1-3) are untouched.
2. **`--debug` is mandatory for the frame evidence.** Without it
   `weston-screenshooter` reports `Output capture error: unauthorized` and writes an
   all-black PNG: weston registers a screenshot authority only when `--debug` is set
   (`frontend/main.c` - `if (debug_protocol) { ... screenshot_allow_all ... }`, whose
   comment is "indiscriminately allow everyone to take screenshots of any output").
   Caveat to document: `--debug` also enables the debug protocol, which weston warns is
   a denial-of-service/information-leak surface. Fine for an ephemeral smoke with a
   private runtime dir and one untrusted client, but it must be stated.
3. **Frame evidence works**: `weston-screenshooter` (client from the same package)
   writes `wayland-screenshot-<timestamp>.png` into its own cwd. With `--debug` the PNG
   is real: 640x480, colourtype 2, **178 distinct sampled colours** (weston's grey
   desktop plus the `weston-flower` window). Without `--debug` the same command yields a
   1289-byte single-colour (`#000000`) PNG.
4. **Reading the pixels needs no new dependency**: a ~40-line stdlib `zlib` PNG reader
   run through the devshell's python3 (`nix develop --command python3`) decodes the
   screenshot. Note for the smoke: a size/non-empty check is *not* sufficient - the
   unauthorised black PNG is a plausible-looking file, so the check must decode pixels
   and look for the pattern colour.
5. **Hard-fail signal for the GL requirement**: forcing GL to fail
   (`__EGL_VENDOR_LIBRARY_FILENAMES=/nonexistent.json`) exits 1 with
   `Error: EGL surfaceless platform cannot be used.` and `fatal: failed to create
   compositor backend`, and creates **no socket** - so the smoke can detect it by
   non-zero exit plus a missing socket, which is exactly the hard-fail behaviour chosen
   while charting.
6. **The pixman opt-in works**: `--renderer=pixman` starts and creates its socket
   (`Using Pixman renderer`, plus a `Color representation not supported by renderer`
   warning), so the planned `--weston-renderer` escape hatch is viable.
7. Lifecycle: weston ignores SIGTERM gracefully (`caught signal 15`); the socket appears
   within ~1s of start.
