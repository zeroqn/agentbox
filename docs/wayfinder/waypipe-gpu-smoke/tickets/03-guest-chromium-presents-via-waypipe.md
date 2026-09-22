---
label: wayfinder:prototype
title: Guest Chromium presents through waypipe
status: closed
blocked_by: ["01-weston-headless-gl-on-host", "02-host-waypipe-client-handshake"]
claimed_by: bob (pi session 2026-09-22)
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

## Prototype (in progress - awaiting your reaction, ticket not yet resolved)

**It works, with the plain flag set.** Evidence in
`/home/dev/loftd/disk/chromium-smoke/t03-plain/` (host) and
`/home/dev/loftd/disk/chromium-smoke/baseline/workspace/evidence/t03-*.txt` (guest).

Setup: weston headless+GL+`--debug` (ticket 01) -> `waypipe -d --socket <D>/waypipe.sock
client` (ticket 02) -> `loftd --mem 4 --gpu=drm --waypipe=<D>/waypipe.sock` -> guest runs

```sh
chromium --ozone-platform=wayland --no-sandbox --disable-gpu-sandbox \
         --use-angle=vulkan --enable-features=Vulkan \
         --user-data-dir=/tmp/c-t03 --window-size=640,480 \
         --app=file:///workspace/smoke/wp-page.html --enable-logging=stderr
```

where `wp-page.html` paints a magenta (`#ff00ff`) page with white text.

1. **The transport carries it.** Two Wayland connections (browser + GPU process) handshake
   with the host client (`Connection received`, `may use dmabufs: true`), and the client's
   request trace shows a real toplevel: `wl_compositor.create_surface`,
   `xdg_toplevel.set_title`/`set_min_size`, and **5x `wl_surface.commit`**.
2. **The pixels arrive.** The screenshot the host compositor took mid-run (640x480) contains
   **13094 sampled pixels of `#ff00ff` plus 2183 of `#ffffff`** - the page's background and
   text - among 315 distinct colours. Compare the rio-only run of ticket 02, which had no
   magenta at all.
3. **`--disable-vulkan-surface` was NOT needed here.** The plain variant (no flag) both
   connected and presented, unlike the headless case where ANGLE's WSI path aborted the GPU
   process. So the presenting run is not gated by that flag; the flag question from the
   earlier diagnosis does not carry over to the waypipe/display case.
4. **The buffer path is `wl_shm`, not dmabuf.** The requests are `wl_shm.create_pool` (4x)
   and `wl_shm_pool.resize` (3x); no dmabuf transfer appears, despite the handshake
   advertising `may use dmabufs: true`.
5. **Caveat - the presenting run probably did not use venus.** The guest Chromium log shows
   `MESA-LOADER: failed to open dri: /run/opengl-driver/lib/gbm/dri_gbm.so: cannot open
   shared object file` and `WARNING ui/ozone/platform/wayland/ozone_platform_wayland.cc:278
   Failed to initialize drm render node handle.`, consistent with a software/immediate
   renderer feeding a shm buffer. Under the charter that is acceptable (the transport is the
   subject and the venus claim is scored by the headless run), but it means this prototype
   does **not** show venus-backed presentation - see the new ticket below.
6. Method note: `T03_VARIANT` did not reach the guest (loftd passes only PATH plus
   `IMAGE_LOFTD_ENV_ALLOWLIST`), so the second, `--disable-vulkan-surface` run silently
   repeated the plain variant. Guest-side variant selection has to travel by another
   channel (e.g. baked into the staged guest script) or not at all.

## Resolution

**Yes - and on venus.** The prototype first proved presentation with the plain flag set and
software/shm buffers; bob's challenge ("I need hardware access for Chromium in the loftd guest
through waypipe") then drove the engine investigation, whose answer (`Venus-backed presenting
run through waypipe`) upgrades this ticket's result. The configuration that both presents and
renders on the GPU:

```sh
# guest, in the waypipe session
GBM_BACKENDS_PATH=/usr/lib/loftd-mesa-runtime/lib/gbm \
chromium --ozone-platform=wayland --no-sandbox --disable-gpu-sandbox \
         --use-angle=vulkan \
         --user-data-dir=/tmp/c --window-size=640,480 \
         --app=file:///workspace/smoke/wp-page.html
```

with the host waypipe client started with dmabuf blocked (`waypipe -n ... client`).

Measured: 0 GPU-process crashes, the GPU process alive, venus in use
(`VIRTGPU_CONTEXT_INIT=5`, `VIRTGPU_EXECBUFFER=42`, `VIRTGPU_MAP=4` under strace), and the
page present in the host compositor's screenshots (`#ff00ff` x13094, `#ffffff` x2183) - both
early and late, so it is a stable window rather than a lucky frame.

Flags that must **not** be used: `--enable-features=Vulkan` (switches the display compositor
to Vulkan, needs a `VkSurfaceKHR` ozone-wayland does not implement; the guest GPU process then
crash-loops 5x and never paints).

Evidence: `/home/dev/loftd/disk/chromium-smoke/{t03-plain,t06-evidence/anglevk,t06-evidence/anglevk-nogpu}/`.

Consequences carried into the design freeze:

- The smoke now needs a guest-side env addition (`GBM_BACKENDS_PATH`) and a waypipe-side
  setting (dmabuf blocked) - both settable from the smoke's staged guest script and the host
  client invocation, no image change required.
- The presenting run is no longer renderer-agnostic in practice: it can carry the venus claim
  itself, which is the scoring question left to `Freeze the --waypipe mode design`.
