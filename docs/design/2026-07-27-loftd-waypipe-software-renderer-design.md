# Loftd Waypipe Software Renderer Design

## Status

Approved on 2026-07-27.

## Problem

`loftd --waypipe` deliberately starts the guest Waypipe server with `--no-gpu` and does not enable libkrun virtio-GPU. Applications that can render directly into Wayland shared-memory buffers, such as Foot, work. Applications that require a graphics API fail because the loftd guest image contains graphics loaders but no Mesa software drivers:

- Ghostty cannot acquire the EGL/OpenGL context required by its GTK frontend.
- Rio cannot initialize a Vulkan device through wgpu.

The pinned Mesa package provides both required CPU renderers:

- llvmpipe through `swrast_dri.so` and Mesa's EGL vendor implementation
- lavapipe through `libvulkan_lvp.so` and `lvp_icd.x86_64.json`

## Goals

- Allow OpenGL/EGL applications such as Ghostty to launch through `loftd --waypipe` using llvmpipe.
- Allow Vulkan applications such as Rio to launch through `loftd --waypipe` using lavapipe.
- Keep Waypipe in `--no-gpu` mode and transport shared-memory Wayland buffers.
- Scope forced software rendering strictly to `--waypipe` launches.
- Preserve hardware-backed behavior for local `--wayland` and `--gpu=drm` launches.
- Detect missing renderer artifacts during image validation rather than adding runtime fallback logic.

## Non-goals

- Enable libkrun virtio-GPU for remote Waypipe launches.
- Forward DRM nodes, DMA-BUF objects, or host GPU acceleration through the remote Waypipe path.
- Change the behavior of local Wayland passthrough.
- Add Ghostty itself to the loftd image.
- Add Ghostty- or Rio-specific runtime branches.
- Globally force software rendering for all loftd guest commands.

## Considered approaches

### Embed Nix store paths in guest-init

Guest-init could receive or compile in the exact Mesa store paths and export them for the Waypipe child.

This keeps the environment narrowly scoped, but couples Rust runtime behavior directly to packaging-specific store hashes or requires expanding the host-to-guest launch contract.

### Stable guest renderer tree with scoped environment

The loftd image provides a stable guest path that points to the complete Mesa package. Guest-init references that path only when wrapping a command with Waypipe.

This preserves Mesa's internal metadata layout, keeps Nix store hashes out of Rust, and leaves other launch modes unchanged.

This is the selected approach.

### Global graphics-driver discovery

The image could expose Mesa through global environment variables or a global OpenGL driver path.

This is simpler, but it can affect local virtio-GPU launches and makes the software renderer part of every guest process environment. It is rejected because the requirement is specific to software-only Waypipe launches.

## Architecture

### Loftd-only image contents

Add `pkgs.mesa` to the loftd-only package list in `nix/image/layers.nix`. The agentbox image remains unchanged.

During loftd image assembly in `nix/image/container.nix`, create this stable link:

```text
/usr/lib/loftd-software-renderer -> ${pkgs.mesa}
```

The link targets the complete Mesa package rather than copying selected files. The pinned Mesa package's EGL vendor and Vulkan ICD metadata reference its libraries by absolute Nix store path, so rooting the complete package preserves those targets while the stable link gives guest-init hash-independent metadata and DRI paths.

The relevant stable paths become:

```text
/usr/lib/loftd-software-renderer/lib/dri/swrast_dri.so
/usr/lib/loftd-software-renderer/lib/libEGL_mesa.so.0
/usr/lib/loftd-software-renderer/share/glvnd/egl_vendor.d/50_mesa.json
/usr/lib/loftd-software-renderer/lib/libvulkan_lvp.so
/usr/lib/loftd-software-renderer/share/vulkan/icd.d/lvp_icd.x86_64.json
```

### Guest Waypipe environment

When `LOFTD_WAYPIPE_PORT` is present, guest-init constructs the Waypipe launch with these child-process environment variables:

```text
LIBGL_ALWAYS_SOFTWARE=1
LIBGL_DRIVERS_PATH=/usr/lib/loftd-software-renderer/lib/dri
__EGL_VENDOR_LIBRARY_FILENAMES=/usr/lib/loftd-software-renderer/share/glvnd/egl_vendor.d/50_mesa.json
VK_DRIVER_FILES=/usr/lib/loftd-software-renderer/share/vulkan/icd.d/lvp_icd.x86_64.json
```

The existing Waypipe invocation remains semantically unchanged:

```text
waypipe --no-gpu --vsock --socket PORT server -- COMMAND...
```

The implementation may express the environment through `/usr/bin/env` in the generated argument vector or through the existing final-exec environment mechanism. The behavior must remain testable as a pure command/environment plan, and the variables must apply only to the Waypipe process and its command child.

When no Waypipe port is present, guest-init launches the requested command without these variables.

### Data flow

- The user invokes `loftd --waypipe=SOCKET -- COMMAND`.
- Loftd validates the forwarded Unix socket and maps it to a guest vsock port.
- Loftd does not enable `GpuMode::Drm` for this launch.
- Guest-init sees `LOFTD_WAYPIPE_PORT` and constructs the software-renderer environment.
- Guest-init starts Waypipe with `--no-gpu`.
- Ghostty discovers Mesa EGL and llvmpipe through the scoped environment.
- Rio discovers the lavapipe Vulkan ICD through the scoped environment.
- The applications render on the guest CPU and submit shared-memory Wayland buffers.
- Waypipe carries the Wayland protocol and buffer contents through the existing vsock and SSH-forwarded transport.

## Failure behavior

Guest-init does not probe for Mesa files and does not select an application-specific fallback. The loftd image contract guarantees that the stable renderer tree and required artifacts exist.

If packaging regresses, image checks fail during build. If a user replaces the guest image with one that violates the contract, the graphics loader reports its normal EGL or Vulkan initialization error.

This avoids masking broken image assembly and avoids maintaining runtime fallback branches for paths that cannot be absent in the supported image.

## Testing

### Guest-init tests

Add or extend tests near the Waypipe command construction to assert:

- A launch with a Waypipe port includes all four software-renderer variables.
- A launch without a Waypipe port includes none of them.
- The existing `waypipe --no-gpu --vsock --socket PORT server -- COMMAND...` arguments remain intact.
- The renderer variables apply to the Waypipe child rather than mutating unrelated launch modes.

### Image checks

Extend `nix/image/checks.nix` for the loftd variant to assert:

- `pkgs.mesa` is rooted by the loftd image definition.
- The image assembly creates `/usr/lib/loftd-software-renderer`.
- Mesa contains `lib/dri/swrast_dri.so`.
- Mesa contains `share/glvnd/egl_vendor.d/50_mesa.json`.
- Mesa contains `lib/libvulkan_lvp.so`.
- Mesa contains `share/vulkan/icd.d/lvp_icd.x86_64.json`.

The agentbox variant must remain free of this loftd-only renderer contract.

### Repository validation

Run:

```bash
nix develop --command cargo fmt --check
nix develop --command cargo clippy --all-targets --all-features -- -D warnings
nix develop --command cargo deny check
nix develop --command cargo test
```

Also build or run the loftd image checks so the filesystem and closure assertions are evaluated.

### Live validation

When an external Waypipe client and forwarded socket are available, launch both applications through the documented workflow:

```bash
loftd --workspace=/absolute/workspace --waypipe=/absolute/forwarded.sock -- ghostty
loftd --workspace=/absolute/workspace --waypipe=/absolute/forwarded.sock -- rio
```

Capture direct application output to confirm:

- Ghostty acquires an EGL/OpenGL context backed by llvmpipe.
- Rio creates a Vulkan CPU device backed by lavapipe.
- Both display through the remote compositor.

Live validation supplements but does not replace the deterministic guest-init and image checks.

## Documentation

Update the Waypipe section in `README.md` to state that:

- `--waypipe` remains software-only and uses `--no-gpu`.
- OpenGL/EGL clients run through Mesa llvmpipe.
- Vulkan clients run through Mesa lavapipe.
- Rendering occurs on the guest CPU and does not use libkrun virtio-GPU acceleration.
