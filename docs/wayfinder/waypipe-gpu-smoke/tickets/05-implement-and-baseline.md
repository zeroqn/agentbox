---
label: wayfinder:task
title: Implement the mode and record its baseline
status: open
blocked_by: ["04-freeze-waypipe-mode-design"]
claimed_by: null
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
