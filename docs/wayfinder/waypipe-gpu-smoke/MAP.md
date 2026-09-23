---
label: wayfinder:map
title: Waypipe + drm=gpu live smoke
---

## Destination

A live smoke in `tools/chromium-cang-smoke` (a new `--waypipe` mode) that proves the
waypipe transport: a guest Chromium presents a frame to a host-side Wayland compositor
through cang's `--waypipe` path, while the same microVM separately proves the hardware
venus renderer. Reaching the destination = the mode exists, passes reproducibly on this
host, is A/B attributable, and its baseline is recorded in the tool's README.

**Status (2026-09-22): reached.** The `--waypipe` mode exists in
`tools/chromium-cang-smoke`, passes reproducibly on this host, is A/B
attributable, and its baseline is recorded in the tool's README. No open tickets;
the two product follow-ons under *Out of scope* remain for their own effort.

## Notes

- Domain: cang host (`crates/cang`), guest bootstrap (`crates/cang-guest-init`), the
  `--gpu=drm` venus render server, and `tools/chromium-cang-smoke`.
- Skills worth consulting: `grilling`, `domain-modeling`, `diagnosing-bugs`.
- Standing preferences for this effort: honest scoring (every check must be able to
  fail); hard failure over silent weakening; reuse the existing smoke's hermetic podman
  store, btrfs preflight, workspace staging and evidence scoring instead of duplicating
  them; document behaviour in `tools/chromium-cang-smoke/README.md`.
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
  current nixpkgs pin, and waypipe already ships in the cang image.

## Decisions so far

<!-- one line per closed ticket, gist plus link -->

- [Freeze the --waypipe mode design](tickets/04-freeze-waypipe-mode-design.md): flags, six stages, evidence files and exact predicates frozen into the tool's README, with the reasons (host-side venus evidence via the window title, pixel-decoded frames, mandatory control, liveness preflights, file-based guest mode, dedicated dwell).
- [Implement the mode and record its baseline](tickets/05-implement-and-baseline.md): `--waypipe` implemented (commit a97767e) and green three times on the pinned artifacts - transport, venus-presenting, frame-presented and control-no-frame all PASS; the checks were also seen to fail, and both hard-fail preflights were exercised.
- [Venus-backed presenting run through waypipe](tickets/07-venus-backed-presenting-run.md): **hardware-accelerated presentation works** - guest Chromium `--ozone-platform=wayland --use-angle=vulkan` (NOT `--enable-features=Vulkan`) + `GBM_BACKENDS_PATH` + dmabuf blocked on the waypipe side gives 0 GPU crashes, venus proven by `VIRTGPU_CONTEXT_INIT`/`EXECBUFFER`, and the page in the host screenshot; transfer is `wl_shm` (not zero-copy).
- [Guest Chromium presents through waypipe](tickets/03-guest-chromium-presents-via-waypipe.md): the working presenting configuration is `--ozone-platform=wayland --use-angle=vulkan` + `GBM_BACKENDS_PATH` + dmabuf blocked; `--enable-features=Vulkan` must not be used (Vulkan display compositing needs a `VkSurfaceKHR` ozone-wayland lacks, and the GPU process then crash-loops and never paints).
- [Guest Chromium GPU process in the waypipe run](tickets/06-guest-chromium-gpu-process-in-waypipe-run.md): there is a real env gap (`GBM_BACKENDS_PATH` set nowhere, so ozone can't load `dri_gbm.so`), but venus-backed presentation is blocked by Chromium itself (`'--ozone-platform=wayland' is not compatible with Vulkan` -> GPU crash loop) and, once buffers are dmabufs, by waypipe's own import failure (`ERROR_INVALID_DRM_FORMAT_MODIFIER_PLANE_LAYOUT_EXT`). The working configuration is the shm/software one that the ticket 03 prototype used.
- [Host waypipe client and vsock handshake](tickets/02-host-waypipe-client-handshake.md): the host runs `waypipe client` on the socket path cang is given (cang preflights it and fails fast if missing or not a socket); the dial is lazy - it happens on the first guest app connection, so a real guest client and the client log's `Connection received`/`may use dmabufs: true` lines are the evidence, not the guest socket's existence; the image's `rio` already painted a window the host compositor captured.
- [weston headless + GL on this host](tickets/01-weston-headless-gl-on-host.md): weston 15.0.1 runs headless+GL on the host GPU (`GL renderer: AMD Radeon RX 7600M XT`, `renderD128`, no DRM master); screenshots need `--debug` (else `Output capture error: unauthorized` and an all-black PNG); `weston-screenshooter` writes a real PNG, decodable with stdlib zlib via the devshell python3; GL failure exits 1 and creates no socket.

## Not yet specified

- Nothing. Every question this effort could state sharply is either answered in
  *Decisions so far* or ruled out under *Out of scope*; the two remaining threads
  are product work with their own risk, not fog on this route.

## Out of scope

- **dmabuf zero-copy** for the presenting run. GPU-accelerated presentation itself is now in
  scope (see *Venus-backed presenting run through waypipe*); what stays out is making the
  transfer zero-copy, which needs modifier negotiation fixed between the guest
  (venus/virtio-gpu) and the host compositor's dmabuf feedback
  (`VK_ERROR_INVALID_DRM_FORMAT_MODIFIER_PLANE_LAYOUT_EXT`). The smoke carries the accelerated
  presenting run on the `wl_shm` path.
- Landing the two product changes this uncovered: adding `GBM_BACKENDS_PATH` to guest-init's
  `MESA_ENV`, and blocking dmabuf for the waypipe server while the modifier gap is open. The
  smoke can set both from its staged guest script and host client invocation; deciding whether
  the image itself should change belongs to the product, not this map.
- Input events (keyboard/pointer) from host to guest: this baseline proves pixels travel.
- weston on a real DRM/KMS output (VT takeover); the compositor is headless by decision.
- Transports other than the vsock path (ssh, plain unix), and X11/XWayland clients.
- Latency or bandwidth benchmarking of the transport.
- Wiring the smoke into `nix build`/CI checks: it needs podman, btrfs and a host GPU,
  exactly like the existing smoke, which is also not a flake check.
