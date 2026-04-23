#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="Yeachan-Heo"
repo="oh-my-codex"
api_url="https://api.github.com/repos/$owner/$repo/releases/latest"
explore_system="x86_64-linux"
explore_asset_name="omx-explore-harness-x86_64-unknown-linux-musl.tar.xz"

for cmd in curl jq nix-prefetch-url nix python3; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "missing required command: $cmd" >&2
    exit 1
  fi
done

prefetch_npm_deps_hash() {
  local lockfile="$1"

  if command -v prefetch-npm-deps >/dev/null 2>&1; then
    prefetch-npm-deps "$lockfile"
  else
    nix run "nixpkgs#prefetch-npm-deps" -- "$lockfile"
  fi
}

release_json="$(curl -fsSL "$api_url")"
version="$(printf '%s' "$release_json" | jq -r '.tag_name' | sed 's/^v//')"

if [ -z "$version" ] || [ "$version" = "null" ]; then
  echo "failed to determine latest oh-my-codex release tag" >&2
  exit 1
fi

archive_url="https://github.com/$owner/$repo/archive/refs/tags/v$version.tar.gz"
mapfile -t prefetch_output < <(nix-prefetch-url --print-path --unpack "$archive_url")
src_hash_base32="${prefetch_output[0]}"
src_path="${prefetch_output[1]}"
src_hash_sri="$(nix hash convert --hash-algo sha256 --to sri "$src_hash_base32")"

if [ -z "$src_path" ] || [ ! -d "$src_path" ]; then
  echo "failed to determine unpacked source path" >&2
  exit 1
fi

lockfile="$src_path/package-lock.json"
if [ ! -f "$lockfile" ]; then
  echo "failed to locate package-lock.json in unpacked source" >&2
  exit 1
fi

npm_deps_hash="$(prefetch_npm_deps_hash "$lockfile" | tail -n 1)"

explore_asset_url="https://github.com/$owner/$repo/releases/download/v$version/$explore_asset_name"
explore_hash_base32="$(nix-prefetch-url "$explore_asset_url")"
explore_hash_sri="$(nix hash convert --hash-algo sha256 --to sri "$explore_hash_base32")"

python3 - "$pins_file" "$version" "$src_hash_sri" "$npm_deps_hash" "$explore_system" "$explore_asset_name" "$explore_hash_sri" <<'PY'
import re
import sys
from pathlib import Path

pins_path = Path(sys.argv[1])
version = sys.argv[2]
src_hash = sys.argv[3]
npm_hash = sys.argv[4]
explore_system = sys.argv[5]
explore_asset = sys.argv[6]
explore_hash = sys.argv[7]
text = pins_path.read_text()

def replace_exact(pattern: str, replacement: str, label: str, source: str | None = None) -> str:
    target = text if source is None else source
    updated, count = re.subn(pattern, replacement, target, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f"failed to update {label}; expected exactly one match")
    return updated

text = replace_exact(
    r'(ohMyCodex = \{\s*version = )"[^"]+"(;)',
    rf'\1"{version}"\2',
    "ohMyCodex version",
)
text = replace_exact(
    r'(ohMyCodex = \{.*?srcHash = )"sha256-[^"]+"(;)',
    rf'\1"{src_hash}"\2',
    "ohMyCodex src hash",
)
text = replace_exact(
    r'(ohMyCodex = \{.*?npmDepsHash = )"sha256-[^"]+"(;)',
    rf'\1"{npm_hash}"\2',
    "ohMyCodex npmDepsHash",
)

oh_my_codex_block_pattern = r'(ohMyCodex = \{.*?\n  \};)'
block_match = re.search(oh_my_codex_block_pattern, text, flags=re.S)
if block_match is None:
    raise SystemExit("failed to locate ohMyCodex block in nix/pins.nix")

oh_my_codex_block = block_match.group(1)
system_pattern = rf'({re.escape(explore_system)} = \{{.*?\n\s+\}};)'
system_match = re.search(system_pattern, oh_my_codex_block, flags=re.S)
if system_match is None:
    raise SystemExit(f"failed to locate explore harness block for {explore_system}")

system_block = system_match.group(1)
system_block = replace_exact(
    r'(asset = )"[^"]+"(;)',
    rf'\1"{explore_asset}"\2',
    f"ohMyCodex explore harness asset for {explore_system}",
    source=system_block,
)
system_block = replace_exact(
    r'(hash = )"sha256-[^"]+"(;)',
    rf'\1"{explore_hash}"\2',
    f"ohMyCodex explore harness hash for {explore_system}",
    source=system_block,
)

oh_my_codex_block = (
    oh_my_codex_block[:system_match.start(1)]
    + system_block
    + oh_my_codex_block[system_match.end(1):]
)
text = text[:block_match.start(1)] + oh_my_codex_block + text[block_match.end(1):]

pins_path.write_text(text)
PY

echo "updated nix/pins.nix:"
echo "  ohMyCodexVersion = \"$version\";"
echo "  hash = \"$src_hash_sri\";"
echo "  npmDepsHash = \"$npm_deps_hash\";"
echo "  $explore_system.asset = \"$explore_asset_name\";"
echo "  $explore_system.hash = \"$explore_hash_sri\";"
