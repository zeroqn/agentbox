# loftd `--gpu=drm` → Venus Vulkan via standalone sandboxed render-server runner

Date: 2026-08-27
Status: Approved (Option A, 2026-08-27)

## Why

`loftd --gpu=drm` currently configures the virtio-gpu with the native-context GL
flag set (`0x583` = USE_EGL|THREAD_SYNC|NO_VIRGL|USE_ASYNC_FENCE_CB|DRM). That
path gives the guest **no Vulkan device** (`vkEnumeratePhysicalDevices => -3,
count=0`), because native-context GL and the venus Vulkan renderer cannot
coexist in one virtio-gpu.

The standalone probe proves the hardware-backed venus path works end-to-end with
flags `0x6c0` (VENUS|NO_VIRGL|RENDER_SERVER|DRM) and a radeon host ICD: the
guest sees `Virtio-GPU Venus (AMD Radeon RX 7600M XT (RADV NAVI33))`,
`vkCreateDevice => 0`, `RESULT: PASS`.

Goal: repurpose `--gpu=drm` to use that proven venus path, with
`virgl_render_server` running as a **separate process** that carries **its own
seccomp + landlock**, so the VM worker's existing sandbox is left untouched.

## Architecture

```
loftd supervisor
├── fork → render-server runner
│        ├── landlock (RO+Execute store paths, /dev/dri device)
│        ├── no_new_privs + seccomp allowlist (render-server.json)
│        └── exec virgl_render_server --socket-fd=N   (child socketpair end)
└── fork → VM worker (existing sandbox unchanged)
         └── krun_set_gpu_options3(ctx, 0x6c0, shm, parent_fd)
             → libkrun config → RutabagaBuilder::build(handler, Some(fd))
             → virglrenderer get_server_fd callback transfers fd
             → venus render server over SOCK_SEQPACKET
```

Key enablers already present in the vendored `deps/libkrun` (loftd fork at
`github.com/zeroqn/libkrun.git`, heads/loftd):

- `src/rutabaga_gfx/src/virgl_renderer.rs:230-248` — the `get_server_fd`
  callback is compiled (feature `virgl_renderer_next`, enabled in
  `devices/Cargo.toml:44`) and returns `cookie.render_server_fd.take()...`
- `RutabagaBuilder::build(fence_handler, rutabaga_server_descriptor)` already
  accepts the fd; only the call sites `virtio_gpu.rs:281,299` hardcode `None`.

## File-by-file changes

### 1. `deps/libkrun` (submodule, loftd fork — left as a submodule)

- `src/libkrun/src/lib.rs` — add `krun_set_gpu_options3(ctx_id, virgl_flags,
  shm_size, render_server_fd) -> i32`; store `gpu_render_server_fd` in the ctx
  config (mirrors `krun_set_gpu_options2` at line 1645).
- `src/vmm/src/resources.rs` — add `gpu_render_server_fd: Option<OwnedFd>`.
- `src/vmm/src/builder.rs` — pass the fd from `vm_resources` into
  `attach_gpu_device(...)` (line ~1041).
- `src/devices/src/virtio/gpu/virtio_gpu.rs` — `create_rutabaga(...)` gains an
  fd parameter; pass `Some(descriptor)` instead of `None` to
  `builder.clone().build(fence, fd)` (lines 281, 299).

### 2. `nix/pkgs/libkrun.nix`

Switch from the prebuilt `fetchurl` release to a source build of
`deps/libkrun` (`rustPlatform.buildRustPackage`; the submodule is already the
loftd fork at `ad8a404`). libkrunfw stays prebuilt. This is the heaviest lift:
no existing cargo2nix/naersk infra exists in this repo, so a clean
`buildRustPackage` recipe with the submodule's `Cargo.lock` is required.

### 3. `crates/loftd/src/runtime/vm/gpu.rs`

Keep `GpuMode::{Off, Drm}`; `Drm`'s meaning changes to "venus Vulkan".
Backward compatible: `parse_config_value` already bails on unknown values;
`off`/`drm` keep parsing.

### 4. `crates/loftd/src/runtime/vm/libkrun/launcher.rs`

- New constant `VIRGLRENDERER_VENUS_FLAGS: u32 = 0x6c0` = VENUS(1<<6) |
  NO_VIRGL(1<<7) | RENDER_SERVER(1<<9) | DRM(1<<10).
- `configure_gpu(GpuMode::Drm)` calls the new `set_gpu_options3(ctx_id,
  0x6c0, 256 MiB, render_server_fd)`.

### 5. `crates/loftd/src/runtime/vm/libkrun/dynamic.rs`

Load `krun_set_gpu_options3` as an optional symbol (same pattern as
`set_gpu_options2`); error if unavailable.

### 6. New: render-server runner (under `crates/loftd/src/runtime/session/supervisor/`)

- `render_server.rs` — creates the SOCK_SEQPACKET socketpair, forks the runner,
  applies the runner's landlock rules + no_new_privs + seccomp (compiled from
  the new policy), then `execv` the `virgl_render_server` binary with
  `--socket-fd=<child_end>`.
- The supervisor keeps the parent end, passes its fd number to the VM worker
  via a new env var (`LOFTD_RENDER_SERVER_FD=N`); the fd survives the helper
  exec + VM-worker fork (socketpair fds are not CLOEXEC by default).
- Runner reaping: the supervisor terminates the runner on VM exit; the render
  server also exits when the client fd (socketpair) closes.
- Store paths for the runner's env (`RENDER_SERVER_EXEC_PATH`,
  `LD_LIBRARY_PATH`, `VK_DRIVER_FILES`) are resolved from the rootfs closure
  (mesa 26.1.8 + vulkan-loader 1.4.341.0 confirmed in the closure) and the
  virglrenderer store path.

### 7. `crates/loftd/src/runtime/session/supervisor/command.rs` (`helper_env`)

When gpu_mode is `Drm`: add `RENDER_SERVER_EXEC_PATH` (libexec path),
`LD_LIBRARY_PATH` (64-bit vulkan-loader + mesa lib dirs, resolved from the
rootfs closure), `VK_DRIVER_FILES` (rootfs `radeon_icd.x86_64.json`), and the
`LOFTD_RENDER_SERVER_FD` number.

### 8. `crates/loftd/src/runtime/landlock.rs`

Reusable rules builder for the runner:

- Path rules (ReadOnly, includes Execute — `AccessFs::{Execute|ReadFile|ReadDir}`):
  virglrenderer store path + its closure, rootfs closure (mesa, vulkan-loader).
- Device rule for `/dev/dri` (radv reaches the L1 host GPU via the virtio-gpu
  DRM capset). Reuses `runtime_device_rules_from` / `PathCategory` patterns.
- VM worker rules unchanged.

### 9. `crates/loftd/assets/seccomp/render-server.json`

Allowlist for the runner (ioctl on DRM, mmap family, shm_open, sendmsg/recvmsg,
futex, eventfd2, poll, openat, membarrier, ...). First version derived
empirically with loftd's existing seccomp audit mode, then hand-trimmed.

### 10. `crates/loftd/src/runtime/launch/config/mod.rs`

No change — `GUEST_GPU_DRM_ENV=1` is already set for `Drm` (line 73), and
guest-init already creates `/dev/dri/renderD128` in the guest.

## Validation

- `nix develop --command cargo fmt --check` / `clippy` / `cargo deny check` /
  `cargo test` (expect 250 passed, 1 pre-existing NixOS `/etc/bash_logout`
  failure — unrelated, do not "fix").
- `nix build .#agentbox` with the source-built libkrun.
- Live smoke `loftd --gpu=drm`:
  - guest `vulkaninfo`/probe shows the RADV venus device;
  - `ps` shows `virgl_render_server` as a separate child;
  - `/proc/<runner-pid>/status` shows `Seccomp: 2` (filter) and landlock rules
    applied;
  - VM worker's sandbox unchanged (its seccomp filter still the default policy).
- Use loftd seccomp audit mode to iterate the runner's syscall set empirically.

## Open risks

- **libkrun source build** is the main risk (no existing infra; needs a working
  `buildRustPackage` recipe for the vendored workspace).
- The exact runner syscall set needs empirical audit-mode iteration (mesa/radv
  may pull in more syscalls than predicted).
- `virgl_render_server --socket-fd=N` parses the option but rejects invalid
  fds ("no valid client fd specified") — must pass a real SOCK_SEQPACKET
  socketpair end.
