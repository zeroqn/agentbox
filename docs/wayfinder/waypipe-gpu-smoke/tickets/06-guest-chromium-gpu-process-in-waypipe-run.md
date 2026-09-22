---
label: wayfinder:research
title: Guest Chromium GPU process in the waypipe run
status: open
blocked_by: []
claimed_by: null
---

## Question

In the ticket 03 prototype the presenting guest Chromium never initialised a GPU device:
`MESA-LOADER: failed to open dri: /run/opengl-driver/lib/gbm/dri_gbm.so: cannot open shared
object file` and `Failed to initialize drm render node handle`, after which its buffers are
`wl_shm`. Yet the same image renders on venus in the headless smoke, where the guest env
carries `LIBGL_DRIVERS_PATH` and friends.

Find out:

1. Which env/paths the *headless* path sets that the *waypipe* path does not (compare the
   guest env in both runs, and `nix/image/config.nix` plus
   `crates/loftd-guest-init/src/guest_init/components/*`).
2. Whether pointing the GBM/dri loader at the guest's mesa (`LIBGL_DRIVERS_PATH`,
   `__EGL_VENDOR_LIBRARY_FILENAMES`, `VK_DRIVER_FILES`) makes the windowed GPU process come
   up on venus, and whether it then presents at all (this may run straight into the
   venus-present wall).
3. Whether it matters for this effort: the charter says the presenting run may be
   renderer-agnostic. The question is only worth answering if it changes a scored check or
   reveals an image env gap that also affects ordinary waypipe use.

## Deliverable

A short verdict plus the env diff, and - if the fix is a one-liner in the image or the smoke
- a recommendation on whether to take it here or leave it to the uv/venus follow-on effort.
