# Maintenance helpers

Pinned-asset refresh scripts for `nix/pins.nix`, run from the dev shell
(`nix develop`).

Refresh pinned cang prebuilt release metadata in `nix/pins.nix` from a neutral
raw-ELF `sha-*` release. The updater rejects wrapper-script assets, legacy
flake-locked names, and payloads containing concrete
`/nix/store/<hash>-...` references:

```bash
nix develop --command ./scripts/update-cang-prebuilt.sh
```

Refresh pinned RTK prebuilt release metadata in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-rtk-prebuilt.sh
```

Refresh pinned Helvesec/rmux prebuilt release metadata in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-rmux-prebuilt.sh
```

Refresh pinned `dolthub/dolt` prebuilt release metadata (tag and per-system
asset hashes) in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-dolt-prebuilt.sh
```

Refresh pinned `gastownhall/beads` prebuilt release metadata (tag and
per-system asset hashes; release asset names embed the tag without its leading
`v`) in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-beads-prebuilt.sh
```

Refresh pinned `@pydantic/monty-linux-x64-gnu` worker metadata (version, tarball
asset name, and SRI hash) in `nix/pins.nix` from the npm registry:

```bash
nix develop --command ./scripts/update-monty-prebuilt.sh
```

Refresh pinned `zeroqn/libkrun` prebuilt release metadata in `nix/pins.nix`
from the newest matching `loftd-*` tag that contains both required Linux assets.
Root `.#libkrun` and every shared consumer (`.#crun`, `.#podman`, `.#cang`, images, and
`.#cang-prebuilt`) use the same pinned prebuilt libkrun
package. Local source builds stay in the submodule-aware dev flake and use the
checked-out `deps/libkrun` submodule:

```bash
nix develop --command ./scripts/update-libkrun.sh
```

Refresh pinned `zeroqn/libkrunfw` release metadata in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-libkrunfw.sh
```

Refresh pinned Pi coding agent source/npm metadata in `nix/pins.nix` from `earendil-works/pi`:

```bash
nix develop --command ./scripts/update-pi-coding-agent.sh
```

Refresh pinned `dirgeSandboxPrebuiltRelease` metadata in `nix/pins.nix` from the
newest `zeroqn/dirge` GitHub release containing the sandboxed dirge prebuilt
asset:

```bash
nix develop --command ./scripts/update-dirge-sandbox-prebuilt.sh
```

Refresh pinned `omp` prebuilt release metadata in `nix/pins.nix` from `can1357/oh-my-pi`:

```bash
nix develop --command ./scripts/update-omp-prebuilt.sh
```

Refresh pinned `herdrdev/herdr` prebuilt release metadata (tag and per-system
asset hashes) in `nix/pins.nix`:

```bash
nix develop --command ./scripts/update-herdr.sh
```

Refresh pinned `zvec-ai/zvec-grep` source and npm dependency metadata in
`nix/pins.nix` (the updater also rejects a release whose `bin.zg` no longer
points at `dist/cli/index.js`, which `nix/pkgs/zvec-grep.nix` installs):

```bash
nix develop --command ./scripts/update-zvec-grep.sh
```
