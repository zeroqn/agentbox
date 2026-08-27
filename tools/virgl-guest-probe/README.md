# Virgl Guest Probe

A bare libkrun microVM that boots a venus virtio-gpu and checks whether the
guest sees a usable Vulkan device through virglrenderer's VENUS renderer.

**Status: PASSING** — after a host-side environment fix (see "Host-side
requirements" below), the guest creates a venus Vulkan instance, enumerates
one physical device (`Virtio-GPU Venus (llvmpipe ...)`), and creates a logical
device.  The host ICD is Lavapipe (software rendering); this is a diagnostic
probe, not a product configuration.

## Files

- `guest-probe.c` — in-guest binary: dlopens `libvulkan.so.1`, enumerates
  physical devices, and creates a logical device.  Compiles against
  `vulkan-headers` but does NOT link libvulkan (it is loaded at runtime so
  the probe always uses whatever Mesa/Vulkan the guest image ships).
- `guest-rootfs.nix` — Nix derivation that assembles a minimal rootfs
  directory tree containing glibc, Mesa (with the venus/virtio ICD), the
  Vulkan loader, busybox, and the baked `guest-probe` binary.  Its `/init`
  mounts `/dev`, `/proc`, `/sys` and runs the probe.
- `launcher.c` — host-side driver: creates a libkrun microVM context, sets
  the rootfs, configures the virtio-gpu with VENUS|NO_VIRGL|RENDER_SERVER
  flags and a 256 MiB SHM window, routes the guest console to a host file,
  and enters the VM.
- `run.sh` — convenience script: builds the rootfs and launcher via `nix
  build`, boots the VM, and greps the console log for the probe verdict.
  **Sets the host-side render-server environment** (LD_LIBRARY_PATH for a
  64-bit vulkan-loader + mesa, VK_DRIVER_FILES → lvp_icd.x86_64.json) so the
  venus render server can open a usable libvulkan on the host.
- `flake.nix` — Nix flake that wires everything together: provides the
  `guest-probe`, `guest-rootfs`, and `launcher` packages, plus a dev shell
  with the required build inputs (gcc, vulkan-headers, libkrun).

## Usage

```bash
# Build the rootfs and launcher, boot the VM, and check the verdict:
nix run .           # or
bash run.sh

# Override GPU flags (default 0x2c0 = VENUS|NO_VIRGL|RENDER_SERVER):
bash run.sh 0x2c0

# Enter the dev shell for manual compile/test:
nix develop . -c bash
```

## Host-side requirements

The venus render server (`virgl_render_server`, a 64-bit process spawned by
libkrun on the host) dlopens `libvulkan.so.1` **by soname** to do the actual
Vulkan work.  Its search is governed by LD_LIBRARY_PATH (its own RUNPATH only
covers gbm/glibc), so two host-side failures are possible:

- `wrong ELF class: ELFCLASS32` — the render server found a **32-bit**
  `libvulkan.so.1` on its search path.  NixOS machines can ship a 32-bit
  `vulkan-loader` (e.g. pulled in by i686 deps of `ffmpeg-headless` or
  `pipewire`); if that dir is on LD_LIBRARY_PATH, the 64-bit server loads the
  32-bit loader and fails.
- `cannot open shared object file` — no `libvulkan.so.1` is findable at all
  (clean environment, since the render server does not search `/run/opengl-driver`).

Both produce the same guest symptom: `vkCreateInstance => -1`.

`run.sh` fixes both by prepending the rootfs closure's 64-bit `vulkan-loader`
and `mesa` lib dirs to LD_LIBRARY_PATH and setting VK_DRIVER_FILES to the
rootfs's `lvp_icd.x86_64.json` (Lavapipe software ICD).  Any product runtime
that spawns this render server must set the same environment.

## Architecture

```
┌─ host ──────────────────────────────────────────────────────┐
│  launcher.c  ────  libkrun  ──── virglrenderer (RENDER_SERVER) │
│     │                       │                                   │
│     │  krun_create_ctx      │  virtio-gpu                       │
│     │  krun_set_root        │  VENUS renderer                   │
│     │  krun_set_gpu_opts2   │                                    │
│     │  krun_start_enter     │                                    │
│     └───────┬───────────────┘                                    │
└─────────────┼────────────────────────────────────────────────────┘
              │
┌─ guest ─────┴──────────────────────────────────────────────────┐
│  /init  ──►  guest-probe  ──►  libvulkan.so.1                 │
│                      │          (virtio ICD)                    │
│                      └──►  vkCreateInstance                     │
│                      └──►  vkEnumeratePhysicalDevices           │
│                      └──►  vkCreateDevice                       │
│                      └──►  "RESULT: PASS"                       │
└─────────────────────────────────────────────────────────────────┘
```