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

## Decisions already taken (inputs, recorded as they land)

- **Bob, 2026-09-22: the presenting run carries the venus claim itself.** The separate
  headless run is no longer the renderer authority; the presenting run must produce the
  venus evidence. (Mechanism below is the recommended implementation, verified in this
  session; the freeze ticket should confirm or replace it.)
- **Recommended mechanism, verified**: make the guest page put the WebGL renderer string into
  the window title, and read it off the **host** waypipe client log. waypipe logs titles
  verbatim:

  ```text
  Processing request: xdg_toplevel#24.set_title("waypipe-present")
  ```

  so a page doing

  ```js
  const gl = document.createElement('canvas').getContext('webgl2');
  const ext = gl.getExtension('WEBGL_debug_renderer_info');
  document.title = 'waypipe-venus:' + gl.getParameter(ext ? ext.UNMASKED_RENDERER_WEBGL : gl.RENDERER);
  ```

  yields a line like `set_title("waypipe-venus:ANGLE (AMD, Vulkan 1.4.334 (Virtio-GPU Venus
  (AMD Radeon RX 7600M XT (RADV NAVI33)), venus)")` in the host client log. That gives, in one
  artefact: proof the title crossed the transport, the venus renderer, and (with the existing
  predicate) absence of SwiftShader. It needs no strace, no debugging port and no published
  port, and does not distort the run the way stracing the GPU process did (strace slowed
  startup enough to hide a crash loop - so do not make strace part of the scored path).
- The presenting run must additionally set the guest env and waypipe flag that make venus
  presentation work at all: `GBM_BACKENDS_PATH=/usr/lib/loftd-mesa-runtime/lib/gbm`, Chromium
  flags `--ozone-platform=wayland --use-angle=vulkan` (never `--enable-features=Vulkan`), and
  dmabuf blocked on the waypipe client (`-n`). See *Venus-backed presenting run through
  waypipe*.
