#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="zeroqn"
repo="libkrun"
ref="loftd"
rev=""
system="${UPDATE_LIBKRUN_SYSTEM:-x86_64-linux}"

usage() {
  cat <<'USAGE_EOF'
Usage: update-libkrun.sh [--rev <revision>] [--ref <ref>] [--system <system>]

Refresh the pinned zeroqn/libkrun source metadata in nix/pins.nix.

Default:
  --ref     loftd
  --system  current updater system, default x86_64-linux

The updater recomputes both the GitHub source SRI hash and the Cargo vendor
SRI hash used by nix/pkgs/libkrun.nix.
USAGE_EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --rev)
      rev="${2:?missing value for --rev}"
      shift 2
      ;;
    --ref)
      ref="${2:?missing value for --ref}"
      shift 2
      ;;
    --system)
      system="${2:?missing value for --system}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

for cmd in curl jq nix nix-prefetch-url python3; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "missing required command: $cmd" >&2
    exit 1
  fi
done

if [ -z "$rev" ]; then
  rev="$(
    curl -fsSL "https://api.github.com/repos/$owner/$repo/commits/$ref" |
      jq -r '.sha // empty'
  )"
fi

if [ -z "$rev" ] || [ "$rev" = "null" ]; then
  echo "failed to resolve libkrun revision for ref: $ref" >&2
  exit 1
fi

nixpkgs_rev="$(jq -r '.nodes.nixpkgs.locked.rev' "$repo_root/flake.lock")"
if [ -z "$nixpkgs_rev" ] || [ "$nixpkgs_rev" = "null" ]; then
  echo "failed to read nixpkgs revision from flake.lock" >&2
  exit 1
fi

src_url="https://github.com/$owner/$repo/archive/$rev.tar.gz"
src_hash_nix32="$(nix-prefetch-url --unpack "$src_url")"
src_hash="$(nix hash convert --hash-algo sha256 --from nix32 --to sri "$src_hash_nix32")"

expr="
let
  pkgs = (builtins.getFlake \"github:NixOS/nixpkgs/$nixpkgs_rev\").legacyPackages.$system;
  src = pkgs.fetchFromGitHub {
    owner = \"$owner\";
    repo = \"$repo\";
    rev = \"$rev\";
    hash = \"$src_hash\";
  };
in pkgs.rustPlatform.fetchCargoVendor {
  inherit src;
  hash = pkgs.lib.fakeHash;
}
"

set +e
cargo_output="$(
  nix build --no-link --extra-experimental-features 'nix-command flakes' --expr "$expr" 2>&1
)"
cargo_status=$?
set -e

if [ "$cargo_status" -eq 0 ]; then
  echo "unexpectedly built Cargo vendor derivation with a fake hash" >&2
  exit 1
fi

cargo_deps_hash="$(printf '%s\n' "$cargo_output" | sed -n 's/^[[:space:]]*got:[[:space:]]*\(sha256-[^[:space:]]*\)$/\1/p' | tail -n 1)"
if [ -z "$cargo_deps_hash" ]; then
  printf '%s\n' "$cargo_output" >&2
  echo "failed to extract Cargo vendor hash" >&2
  exit 1
fi

python3 - "$pins_file" "$owner" "$repo" "$rev" "$src_hash" "$cargo_deps_hash" <<'PY_EOF'
import re
import sys
from pathlib import Path

pins_path = Path(sys.argv[1])
owner = sys.argv[2]
repo = sys.argv[3]
rev = sys.argv[4]
src_hash = sys.argv[5]
cargo_deps_hash = sys.argv[6]
text = pins_path.read_text()

block_match = re.search(
    r'libkrunSource = \{\n(?P<body>.*?)\n  \};',
    text,
    re.S,
)
if block_match is None:
    raise SystemExit("failed to locate libkrunSource block in nix/pins.nix")

body = block_match.group("body")
replacements = {
    "owner": owner,
    "repo": repo,
    "rev": rev,
    "srcHash": src_hash,
    "cargoDepsHash": cargo_deps_hash,
}
for key, value in replacements.items():
    body, count = re.subn(
        rf'{key} = "[^"]+";',
        f'{key} = "{value}";',
        body,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"failed to update {key} in libkrunSource block")

updated = text[: block_match.start("body")] + body + text[block_match.end("body") :]
pins_path.write_text(updated)
PY_EOF

cat <<REPORT_EOF
updated nix/pins.nix:
  libkrunSource.owner = "$owner";
  libkrunSource.repo = "$repo";
  libkrunSource.rev = "$rev";
  libkrunSource.srcHash = "$src_hash";
  libkrunSource.cargoDepsHash = "$cargo_deps_hash";
REPORT_EOF
