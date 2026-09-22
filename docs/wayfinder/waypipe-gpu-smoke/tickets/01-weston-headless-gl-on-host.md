---
label: wayfinder:research
title: weston headless + GL on this host
status: open
blocked_by: []
claimed_by: null
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
