---
label: wayfinder:task
title: Freeze the --waypipe mode design
status: open
blocked_by: ["03-guest-chromium-presents-via-waypipe"]
claimed_by: null
---

## Question

Freeze the shape of the `--waypipe` mode in `tools/chromium-loftd-smoke` so
implementation is mechanical:

- New flags and their defaults (`--waypipe` toggle, `--weston`, `--waypipe-bin`,
  `--weston-renderer`, private `XDG_RUNTIME_DIR`), in the style of the existing
  `--loftd/--guest-init/--container` options.
- Stages and their order: preflight (btrfs, free space, weston/waypipe present - hard
  fail if missing), start weston headless+GL, start the waypipe client, run the A/B pair
  (with and without `--waypipe`) in the same VM image, tear everything down even on
  failure.
- Evidence files and their exact scored predicates: host screenshot must contain the
  pattern colour (fresh, non-blank); an INFO record for the compositor log. **Revised while
  charting:** since *Venus-backed presenting run through waypipe* showed the presenting run
  can use venus, decide whether the presenting run should also assert the venus renderer
  (via the guest's `VIRTGPU_*` ioctls or an ANGLE/Vulkan log line) instead of always leaning
  on the separate headless run.
- How the A/B pair is reported so a PASS is unambiguously attributable to the waypipe
  path.

## Deliverable

The agreed spec written as a section of `tools/chromium-loftd-smoke/README.md`
(no implementation in this ticket).
