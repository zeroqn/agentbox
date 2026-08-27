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
- RESOLVED: the render server (virgl_render_server, 64-bit) dlopens
  `libvulkan.so.1` by soname; its search is governed by LD_LIBRARY_PATH.  This
  NixOS machine ships a 32-bit vulkan-loader
  (`/nix/store/1ajccbq...-vulkan-loader-1.4.341.0/lib`, pulled in by i686 deps
  of ffmpeg-headless/pipewire).  When that dir sat on the render server's
  LD_LIBRARY_PATH, the 64-bit server loaded the 32-bit loader →
  `wrong ELF class: ELFCLASS32` → venus fails: vkCreateInstance => -1.
  With a clean env the same path fails as `cannot open shared object file`.
- FIXED in run.sh: prepend the rootfs closure's 64-bit vulkan-loader + mesa to
  LD_LIBRARY_PATH and set VK_DRIVER_FILES to a host ICD from the same mesa.
- HARDWARE BACKEND FOUND: on this L1, radv (radeon_icd) reaches the real GPU
  through the virtio-gpu **DRM capset** (SUPPORTED_CAPSET_IDs = 0x46 → bits
  {1,2,6} = virgl, virgl2, DRM — no venus, but DRM lets radv through).
  Verified with radeon_icd: vkCreateInstance => 0, 1 physical device
  ("Virtio-GPU Venus (AMD Radeon RX 7600M XT (RADV NAVI33))"), 5 queue
  families, vkCreateDevice => 0, RESULT: PASS (2/2 runs).  run.sh now
  defaults to radeon_icd and falls back to lvp_icd.
- without RENDER_SERVER flag (0xc0) venus initializes but
  vkEnumeratePhysicalDevices => -3 (VK_ERROR_INITIALIZATION_FAILED), count=0

Net finding: the bare libkrun guest NOW gets a usable venus Vulkan device,
**hardware-backed** (host ICD = radv → L1 virtio-gpu DRM capset → L0 GPU).
The lavapipe fallback covers hosts with no hardware path.  The render-server
host-ICD choice is the next product decision: loftd must set the same
LD_LIBRARY_PATH + VK_DRIVER_FILES when spawning the render server.

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

All five issues are confirmed `[done]` on the board.  The ELFCLASS32 block is
resolved, the probe reports RESULT: PASS, and — since the L1 virtio-gpu
exposes a DRM capset — the render server now uses radv (hardware) by default,
so the guest venus device is backed by the real AMD GPU.

Follow-up (product, not probe): loftd's `GpuMode` is still only `Off | Drm`
(`crates/loftd/src/runtime/vm/gpu.rs`); a venus mode does not exist in the
product yet.  When it is added, the render-server environment fix
(LD_LIBRARY_PATH + VK_DRIVER_FILES) from run.sh must be carried into the loftd
host runtime, or the L2 venus path will hit the same ELFCLASS32 / missing-libvulkan
failure.  The nested HOST3D ring-shmem mmap claim in conclusion.md also needs
re-examination: this single-libkrun probe maps HOST3D blobs fine with both
lavapipe and radv host devices.