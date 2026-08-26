# Virgl Guest Probe

A bare libkrun microVM that boots a venus virtio-gpu and checks whether the
guest sees a usable Vulkan device through virglrenderer's VENUS renderer.

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