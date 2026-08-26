# Loop Plan: virgl-guest-probe pipeline

Goal: Complete 5 issues in the bare libkrun VM probe pipeline.

## Pipeline order

1. **drg-8b48** (done) → Write `guest-probe.c` (in-guest Vulkan device enumeration + create)
2. **drg-85a5** (done) → Write `guest-rootfs.nix` (init + glibc + mesa venus stack + baked probe binary)
3. **drg-401f** (done) → Write `launcher.c` (bare libkrun VM: create_ctx, set_root, gpu_options2, start_enter)
4. **drg-a904** (done) → Write `flake.nix`, `run.sh`, `README.md`, `.gitignore`
5. **drg-f41b** (done) → Validate: compile launcher, build rootfs, boot VM and check guest verdict

## Status

- [x] drg-8b48: Write guest-probe.c
- [x] drg-85a5: Write guest-rootfs.nix
- [x] drg-401f: Write launcher.c
- [x] drg-a904: Write flake.nix, run.sh, README.md, .gitignore
- [x] drg-f41b: Validate end-to-end

## Validation results (drg-f41b)

All three packages build (`nix build .#guest-probe|launcher|guest-rootfs`).

Boot pipeline works end-to-end:
- launcher boots a bare libkrun VM with venus virtio-gpu (flags 0x2c0)
- guest rootfs `/init` runs (busybox applet symlinks added so `#!/bin/sh` resolves)
- guest-probe runs, dlopens libvulkan.so.1, creates a VkInstance
- `vkGetInstanceProcAddr` must be called with the created instance for
  instance-level functions (NULL only returns global-level) — fixed in probe
- krun_set_exec must be called with `/init` and an EMPTY argv array
  (argv is the additional args, not including exec path; passing the exec
  path again makes the shell treat it as a script arg → "syntax error")
- render server spawns but cannot open a usable libvulkan on the host
  (`wrong ELF class: ELFCLASS32`), so venus fails: vkCreateInstance => -1
- without RENDER_SERVER flag (0xc0) venus initializes but
  vkEnumeratePhysicalDevices => -3 (VK_ERROR_INITIALIZATION_FAILED), count=0

Net finding: the guest does NOT get a usable venus Vulkan device in this
environment; the render-server host libvulkan mismatch is the visible block.
This matches the known-unresolved L2 libkrun/guest GPU boundary.

## Bugs found

- guest-probe.c used vkGetInstanceProcAddr(NULL, ...) for instance-level
  functions → always NULL; must use the created VkInstance. FIXED.
- launcher.c krun_set_exec argv[0] duplicated the exec path → guest shell
  "syntax error: unterminated quoted string". Empty argv FIXED.
- guest-rootfs.nix lacked /bin/sh for the init shebang → "Couldn't execute
  '/bin/sh'". Busybox applet symlinks added. FIXED.
- flake.nix fileset list → nixos-26.05 needs fileset.unions. FIXED.
- run.sh greps console for RESULT; the probe prints the verdict.

---

## Pipeline complete — all 5 issues done

All five issues are confirmed `[done]` on the board. No further work needed.