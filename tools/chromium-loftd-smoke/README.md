# Chromium Loftd Live Smoke

A single-command, artifact-correct live smoke for the loftd GPU path using a
real Chromium inside a real loftd microVM.

The browser is **inside the loftd image** (`browserImageLayer`, built from the
pinned nixpkgs `ungoogled-chromium` wrapped package), so nothing is mounted
from the host and resource files (`*.pak`, libraries) mmap from the
digest-keyed image rootfs — the host-/nix overlay **lowerdir** — instead of a
fuse-overlayfs/virtio-fs upper, which is what previously made Chromium fail to
load resources. The environment is otherwise reproduced by the repository
inputs; the one host prerequisite is a btrfs output filesystem and an
amdgpu-backed DRM render node (see Prerequisites).

## Baseline status (2026-09-22)

Measured against the pinned `pins.libkrunRelease` (`loftd-3842e7383799`) and
the packaged `.#loftd-prebuilt` 0.6.6 — the repo's reproducible starting point,
**not** the uncommitted `deps/libkrun` GPU experiments. Two identical runs:

```text
PASS  version       Chromium 153.0.8010.52
PASS  chromium-rc   gpu-dom=0 webgl=0 dom=0  (all three runs exit 0)
FAIL  webgl-vulkan  renderer=no-webgl
PASS  webgl-png     non-empty screenshot
VERDICT: FAIL
```

The guest boots, Chromium runs, and `gpu-diag.txt` shows the guest does see
`/dev/dri/renderD128` plus the venus ICD
(`VK_DRIVER_FILES=.../virtio_icd.x86_64.json`, `LOFTD_GPU_DRM=1`). The failure
is inside Chromium: the GPU process aborts on every attempt
(`content/browser/gpu/gpu_process_host.cc: GPU process exited unexpectedly:
exit_code=6`) and no WebGL context is created. **GPU acceleration does not work
on this baseline**; flipping this FAIL to PASS is the goal of the follow-on
libkrun work.

Reproducing a pinned baseline (rooted so a later `nix-collect-garbage` cannot
delete the artifacts mid-run):

```bash
nix build .#container      -o roots/container
nix build .#loftd-prebuilt -o roots/loftd-prebuilt
nix build .#agentbox-musl  -o roots/agentbox-musl
tools/chromium-loftd-smoke/chromium-smoke.sh \
  --loftd      "$PWD/roots/loftd-prebuilt/bin/loftd" \
  --guest-init "$PWD/roots/agentbox-musl/bin/loftd-guest-init" \
  --container  "$PWD/roots/container" \
  --out-dir /path/on/btrfs/chromium-smoke --mem 4 --timeout 900
```

## Recreate

```bash
tools/chromium-loftd-smoke/chromium-smoke.sh
```

or with overrides:

```bash
tools/chromium-loftd-smoke/chromium-smoke.sh \
  --loftd /path/to/loftd \
  --guest-init /path/to/loftd-guest-init \
  --container /nix/store/...-loftd-image.tar.gz \
  --mem 4 --timeout 600
```

## What it does

1. **Builds/loads** `.#container` (the flake build runs the image wrapper
   contracts incl. `browserContracts`). The OCI archive is loaded into a
   **hermetic** podman/buildah storage (a fresh `btrfs`-driver store under the
   output dir, with local `TMPDIR`), so the smoke never depends on the ambient
   `~/.config/containers/storage.conf`. The `btrfs` driver is required, not
   `vfs`: loftd snapshots the Buildah-mounted rootfs, and a `vfs` graphroot is
   plain directories that `btrfs subvolume snapshot` rejects.
2. **Resolves** loftd (`.#loftd`) and the guest-init override
   (`.#agentbox-musl` → `bin/loftd-guest-init`) as packaged artifacts.
3. **Isolates** config+state: `XDG_CONFIG_HOME=<out>/config`,
   `XDG_STATE_HOME=<out>/state`; a private `loftd.toml` sets
   `[state].location` and `[task-rootfs].backend = "btrfs-snapshot"`. The
   smoke **fails fast** if the graphroot or state home is not on btrfs, because
   the only implemented task-rootfs backend snapshots the Buildah graphroot.
4. **Stages** `/workspace` = `<out>/workspace` containing only
   `smoke/run-guest.sh` and a **fresh** `evidence/` dir (previous evidence is
   deleted unless `--keep-evidence`). Never reuses stale artifacts.
5. **Launches** the real VM from inside the workspace via a PTY
   (`script -q -e -c`), with the verified smoke shape:
   `loftd --gpu=drm --alloc hardened --mem <n> --seccomp=off --landlock=off
   -- sh /workspace/smoke/run-guest.sh`, console captured to
   `<out>/logs/loftd.console`, under `timeout`.
6. **Scores** fresh evidence (every file must be non-empty **and** have
   mtime ≥ run start, so stale evidence can never pass):

   - `version.txt` — `Chromium <n>`
   - `webgl-dom.html` — post-script DOM of the probe page (holds the runtime
     `renderer=` string)
   - `webgl-renderer.txt` — the runtime `renderer=` / `vendor=` / `gl_version=`
     lines from `webgl-dom.html`; must name `Vulkan` and must not be
     `SwiftShader` (this is the vulkan/venus assertion)
   - `webgl.png` — non-empty PNG (GPU-composited screenshot)
   - `chromium-rc` — `gpu-dom=0 webgl=0 dom=0`, every chromium run rc 0
   - `gpu-dom.html` — the `chrome://gpu` dump, captured for humans but **not**
     scored: its feature-status table is shadow DOM, which `--dump-dom` does
     not serialize.

   The mtime threshold is `date +%s` (seconds), matching `stat -c %Y`; the
   renderer is read from the runtime DOM, never from the static probe HTML.
   Each in-guest chromium invocation is bounded by `timeout 150` so a
   venus/render-server stall is attributed to one run instead of hanging until
   the host-side VM timeout.

   Exit 0 only when all pass; otherwise exit 1 with the failed check and
   evidence/log paths.

## Output layout

```text
<out>/
  config/loftd/loftd.toml   hermetic loftd config
  state/                    loftd state root
  workspace/                guest /workspace (bind)
    smoke/run-guest.sh      the in-guest workload
    evidence/               the scored artifacts
  logs/loftd.console        full VM console
  logs/*                    mirrored chromium logs live in evidence/
```

## Prerequisites

- `/dev/kvm` accessible to the user.
- A `/dev/dri/renderD*` node accessible to the user, backed by an amdgpu host
  GPU exposed through DRM native context: the render-server wrapper pins mesa's
  `radeon_icd`, and loftd runs `virgl_render_server` with venus.
- `podman`, `buildah`, `script` (util-linux), `nix` on PATH (the `nix develop`
  shell provides them; the README live-run section lists the same set).
- **btrfs output filesystem**: both the hermetic container graphroot and the
  loftd state home live under `--out-dir`, and `btrfs-snapshot` is the only
  implemented task-rootfs backend. On non-btrfs the smoke exits 2 up front.
- **Disk space**: the loftd image (with the in-image Chromium) decompresses to
  ~7.7 GiB. The smoke keeps a 12 GiB free-space preflight on the output
  filesystem and fails fast with a clear message rather than mid-load ENOSPC.

## Triage

- **All PASS** — fresh evidence in `<out>/workspace/evidence/` proves Chromium
  launched with the GPU path, WebGL is hardware-backed (non-SwiftShader), and
  a GPU-composited PNG rendered.
- **FAIL webgl-vulkan** — `webgl-renderer.txt` holds `renderer=no-webgl`
  (Chromium could not create any WebGL context), named `SwiftShader`, omitted
  `Vulkan`, or is empty (venus stall). Read `<out>/logs/loftd.console`,
  `<out>/workspace/evidence/gpu-diag.txt`, `chromium-webgl.log`,
  `chromium-webgl-dom.log`, and `webgl-renderer.txt`. The current baseline is
  exactly this case: the Chromium GPU process aborts (`exit_code=6`) and
  `renderer=no-webgl`. The runner's job is reproducibility and honest
  attribution, not to fix Chromium; do not weaken the assertions to force
  green.
- **VM timeout (124)** — `<out>/logs/loftd.console` ends without the guest
  evidence; check host prerequisites (`/dev/kvm`, state-home backend) first.
- **Stale/missing evidence** — the freshness rule caught a reused artifact;
  re-run without `--keep-evidence`.

## Notes

- The default launch flags mirror the running chromium GPU investigation
  (`--seccomp=off --landlock=off` avoids host-policy SIGSYS interference while
  reproducing the GPU evidence; re-enabling host policy is a separate
  hardening task). `--alloc hardened` is required: mimalloc is incompatible
  with Chromium partition_alloc.
- `--no-default-flags` drops the default set except `--mem`, for manual
  experimentation.