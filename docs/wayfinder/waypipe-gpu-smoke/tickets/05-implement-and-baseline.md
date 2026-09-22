---
label: wayfinder:task
title: Implement the mode and record its baseline
status: closed
blocked_by: ["04-freeze-waypipe-mode-design"]
claimed_by: bob (pi session 2026-09-22)
---

## Question

Implement the `--waypipe` mode per the frozen design, then prove it: run the A/B pair
several times on this host, confirm the failures can fail (for example with weston
stopped), and record the baseline, the repro command and any host prerequisites in
`tools/chromium-loftd-smoke/README.md`.

If the prototype surfaced real work in loftd or guest-init, that work graduates to its
own ticket before this one proceeds - this ticket is the smoke, not the runtime fix.

## Deliverable

The implemented mode, N-run evidence, and the documented baseline.

## Resolution

Implemented and baselined.

**Implementation** (commit `a97767e`, 5 files, +569/-7):

- `chromium-smoke.sh`: the `--waypipe` mode - flags, preflight, weston and
  waypipe-client lifecycle with an EXIT trap, run A (waypipe) and run B (control),
  host compositor captures, and the four new scored checks.
- `smoke/run-guest.sh`: the presenting run (driven by the staged
  `smoke/run-mode` file), with `GBM_BACKENDS_PATH`, `--use-angle=vulkan`, and a
  dedicated dwell window before the headless checks.
- `smoke/waypipe-present.html`: the pattern page that also publishes the WebGL
  renderer as the window title.
- `png-colour-count.py`: stdlib-only PNG pixel counter for the frame check.
- `README.md`: the frozen design, the baseline, prerequisites and triage.

**Baseline** (pinned `roots/loftd-prebuilt` 0.6.6 + `roots/agentbox-musl` +
`roots/container`, host amdgpu): `VERDICT: PASS`, three consecutive runs (two
before the final two edits, one after), all nine checks green:

```text
PASS  version / chromium-rc / webgl-vulkan / webgl-png
PASS  waypipe-transport   guest waypipe server connected to the host client
PASS  venus-presenting    waypipe-venus:ANGLE (AMD, Vulkan 1.4.334 (Virtio-GPU Venus (AMD Radeon...
PASS  frame-presented     host compositor screenshot holds 210047 pattern pixels
PASS  control-no-frame    without --waypipe the same work delivers 0 pattern pixels
```

**The checks were seen to fail**, which is what makes the green meaningful:

- `waypipe-transport` FAILed during development when a stale socket from a reused
  `--out-dir` killed the client with `EADDRINUSE` (the preflight had accepted the
  leftover socket file) - fixed by clearing stale sockets and checking liveness.
- `frame-presented` and `venus-presenting` FAILed when the pattern page was not
  staged into the workspace, so the browser showed an error page.
- The two hard-fail preflights were exercised directly: a missing weston binary
  exits 2 with the `nix build nixpkgs#weston` hint, and a weston that starts but
  never creates its socket exits 2 with the `--debug`/`--weston-renderer=pixman`
  hints.

**Not done here** (recorded as out of scope on the map): dmabuf zero-copy, and
landing the two product changes the work uncovered (`GBM_BACKENDS_PATH` in
guest-init's `MESA_ENV`; blocking dmabuf for the waypipe server while the
modifier gap is open). The smoke sets both from its own staged files, so no image
change is required for the baseline to hold.
