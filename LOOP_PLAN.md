# Loop Plan: virgl-guest-probe pipeline

Goal: Complete 5 issues in the bare libkrun VM probe pipeline.

## Pipeline order

1. **drg-8b48** (in_progress) → Write `guest-probe.c` (in-guest Vulkan device enumeration + create)
2. **drg-85a5** (open) → Write `guest-rootfs.nix` (init + glibc + mesa venus stack + baked probe binary)
3. **drg-401f** (open) → Write `launcher.c` (bare libkrun VM: create_ctx, set_root, gpu_options2, start_enter)
4. **drg-a904** (open) → Write `flake.nix`, `run.sh`, `README.md`, `.gitignore`
5. **drg-f41b** (open) → Validate: compile launcher, build rootfs, boot VM and check guest verdict

## Status

- [x] drg-8b48: Write guest-probe.c (verified: gcc -Wall compile OK vs vulkan-headers 1.4.341.0)
- [x] drg-85a5: Write guest-rootfs.nix
- [x] drg-401f: Write launcher.c
- [x] drg-a904: Write flake.nix, run.sh, README.md, .gitignore (flake show evaluates all packages)
- [ ] drg-f41b: Validate end-to-end (in progress)

## Bugs found

(none yet)