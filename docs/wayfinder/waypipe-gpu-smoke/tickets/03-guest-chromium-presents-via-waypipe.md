---
label: wayfinder:prototype
title: Guest Chromium presents through waypipe
status: open
blocked_by: ["01-weston-headless-gl-on-host", "02-host-waypipe-client-handshake"]
claimed_by: null
---

## Question

Does a non-headless guest Chromium on the waypipe display actually paint to the host
compositor, and with which flags?

Prototype the whole chain by hand: weston (headless, GL) on the host, `waypipe client`
listening on a private socket, `loftd --gpu=drm --waypipe=<socket>` with a guest script
that runs Chromium against `WAYLAND_DISPLAY=loftd-waypipe-0` with
`--ozone-platform=wayland` and a page that paints a distinctive colour, then take the
host compositor's screenshot and check the colour is present.

Answer with evidence:

1. The working guest Chromium flag set (does it need `--disable-vulkan-surface`, does
   `--use-angle=vulkan` work at all here, or does the presenting run fall back to a
   non-venus renderer - which is acceptable, since the transport is the subject).
2. Whether the frame arrives at all: screenshot from the host compositor showing the
   pattern, plus the client/compositor logs.
3. Any defect this surfaces in loftd's `--waypipe` path or guest-init's waypipe service
   (readiness, socket ownership, `--no-gpu` negotiation). If it is real work, do not fix
   it here: record it and let it graduate to its own ticket.

## Deliverable

A prototype transcript plus the flag set, screenshot evidence, and any defect found.
