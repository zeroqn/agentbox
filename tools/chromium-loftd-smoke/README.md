# Chromium Loftd Live Smoke

A single-command, artifact-correct live smoke for the loftd GPU path using a
real Chromium inside a real loftd microVM.

The browser is **inside the loftd image** (`browserImageLayer`, built from the
pinned nixpkgs `ungoogled-chromium` wrapped package), so nothing is mounted
from the host and resource files (`*.pak`, libraries) mmap from the
digest-keyed image rootfs — the host-/nix overlay **lowerdir** — instead of a
fuse-overlayfs/virtio-fs upper, which is what previously made Chromium fail to
load resources. The environment is fully reproduced by the repository inputs;
no remembered host setup is required.

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
   **hermetic** podman/buildah storage (a fresh `vfs` store under the output
   dir, with local `TMPDIR`), so the smoke never depends on the ambient
   `~/.config/containers/storage.conf` — which on this machine points at a
   btrfs path that is not actually btrfs.
2. **Resolves** loftd (`.#loftd`) and the guest-init override
   (`.#agentbox-musl` → `bin/loftd-guest-init`) as packaged artifacts.
3. **Isolates** config+state: `XDG_CONFIG_HOME=<out>/config`,
   `XDG_STATE_HOME=<out>/state`; a private `loftd.toml` sets
   `[state].location` and an explicit `[task-rootfs].backend`
   (`fuse-overlay` when the state-home is not on btrfs, else `btrfs-snapshot`).
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
   - `gpu-dom.html` — `chrome://gpu` DOM (requires `--allow-chrome-scheme-url`;
     an exit-0 without DOM is not GPU evidence)
   - `webgl-dom.html` — post-script DOM of the probe page (holds the runtime
     `renderer=` string)
   - `webgl-renderer.txt` — `renderer=` string extracted from `webgl-dom.html`,
     must not be `SwiftShader`
   - `webgl.png` — non-empty PNG (GPU-composited screenshot)
   - `chromium-rc` — both chromium invocations exit 0

   The mtime threshold is `date +%s` (seconds), matching `stat -c %Y`; the
   renderer is read from the runtime DOM, never from the static probe HTML.

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
- `podman`, `buildah`, `script` (util-linux), `nix` on PATH (the `nix develop`
  shell provides them; the README live-run section lists the same set).
- A writable state-home (btrfs for the `btrfs-snapshot` backend, else
  `fuse-overlay` is chosen automatically).
- **Disk space**: the loftd image (with the in-image Chromium) decompresses to
  ~7.7 GiB. The smoke needs that for the hermetic `vfs` load **plus** loftd's
  own image-sync materialization, so keep ~20 GiB free on the output filesystem
  or the load will fail with ENOSPC (the runner preflights and fails fast with
  a clear message).

## Triage

- **All PASS** — fresh evidence in `<out>/workspace/evidence/` proves Chromium
  launched with the GPU path, WebGL is hardware-backed (non-SwiftShader), and
  a GPU-composited PNG rendered.
- **FAIL gpu-dom / webgl** with a stuck VM — read `<out>/logs/loftd.console`
  and `<out>/workspace/evidence/chromium-gpu.log`. This is the open thread:
  Chromium may still fail to fully initialize the venus GPU process in the
  real product VM. The runner's job is reproducibility and honest attribution,
  not to fix Chromium; do not weaken the assertions to force green.
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