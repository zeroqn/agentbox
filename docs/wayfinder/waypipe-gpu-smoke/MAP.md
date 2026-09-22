---
label: wayfinder:map
title: Waypipe + drm=gpu live smoke
---

## Destination

A live smoke in `tools/chromium-loftd-smoke` (a new `--waypipe` mode) that proves the
waypipe transport: a guest Chromium presents a frame to a host-side Wayland compositor
through loftd's `--waypipe` path, while the same microVM separately proves the hardware
venus renderer. Reaching the destination = the mode exists, passes reproducibly on this
host, is A/B attributable, and its baseline is recorded in the tool's README.

## Notes

- Domain: loftd host (`crates/loftd`), guest bootstrap (`crates/loftd-guest-init`), the
  `--gpu=drm` venus render server, and `tools/chromium-loftd-smoke`.
- Skills worth consulting: `grilling`, `domain-modeling`, `diagnosing-bugs`.
- Standing preferences for this effort: honest scoring (every check must be able to
  fail); hard failure over silent weakening; reuse the existing smoke's hermetic podman
  store, btrfs preflight, workspace staging and evidence scoring instead of duplicating
  them; document behaviour in `tools/chromium-loftd-smoke/README.md`.
- **Execution is in scope** for this map: it ends with the mode implemented, passing, and
  its baseline documented, not merely with decisions taken.
- Harness note: `rlm.spawn` children in this session came back tool-less (they could not
  read files or run commands), so the `research` tickets below are worked by a
  tool-capable session rather than by fired subagents.
- Vocabulary settled while charting: the smoke's subject is the **waypipe transport**
  (guest to host frame delivery). **Venus present** (a guest `VkSurfaceKHR` over venus)
  is out of scope - the host GPU cannot see the guest's `wl_display`, which is the same
  wall that forced `--disable-vulkan-surface` in the headless smoke.
- Charter decisions taken while charting (inputs to the tickets, not tickets themselves):
  the compositor is **weston on the host, headless backend, GL renderer**; the smoke owns
  starting and stopping weston and the waypipe client (`--weston` / `--waypipe` flags,
  PATH defaults) and **hard-fails** when either is missing; the frame evidence is a
  host-side screenshot whose pattern colour must be present (fresh, non-blank); the
  existing headless venus renderer check stays as it is; an **A/B pair** (with and without
  `--waypipe`) makes a PASS attributable; if weston's GL renderer cannot initialise the
  smoke fails rather than degrading. Both weston 15.0.1 and waypipe 0.11.0 resolve in the
  current nixpkgs pin, and waypipe already ships in the loftd image.

## Decisions so far

<!-- one line per closed ticket, gist plus link -->

## Not yet specified

- Whether `--waypipe` needs a fix in loftd or guest-init. The guest side is already
  implemented (`waypipe --vsock --socket <PORT> --display loftd-waypipe-0 server -- sleep
  infinity`, with `--no-gpu` only when `LOFTD_GPU_DRM` is unset) but nothing has ever
  driven it end to end, so the first spike may surface a real defect (socket ownership,
  vsock port registration, `--no-gpu` negotiation, readiness timing). Not sharp enough to
  ticket until the spike reports.
- Whether the guest-to-host buffer path is shm or dmabuf, and whether asserting dmabuf
  (zero-copy) should become a second scored check.
- Whether a guest Chromium can use venus *and* present. If the spike shows it can, the
  venus-present gap shrinks and no follow-on effort is needed; if it cannot, that
  limitation needs its own effort (not this map).
- Exact guest Chromium flag set (the spike decides it; `--disable-vulkan-surface` is a
  candidate, and with the transport as the subject the presenting run may legitimately
  use a non-venus renderer).

## Out of scope

- Input events (keyboard/pointer) from host to guest: this baseline proves pixels travel.
- weston on a real DRM/KMS output (VT takeover); the compositor is headless by decision.
- Transports other than the vsock path (ssh, plain unix), and X11/XWayland clients.
- Latency or bandwidth benchmarking of the transport.
- Wiring the smoke into `nix build`/CI checks: it needs podman, btrfs and a host GPU,
  exactly like the existing smoke, which is also not a flake check.
