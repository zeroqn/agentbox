#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
flake_file="$repo_root/flake.nix"
owner="Yeachan-Heo"
repo="oh-my-codex"
api_url="https://api.github.com/repos/$owner/$repo/releases/latest"

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

python3 - "$flake_file" "$version" "$src_hash_sri" "$npm_deps_hash" <<'PY'
import re
import sys
from pathlib import Path

flake_path = Path(sys.argv[1])
version = sys.argv[2]
src_hash = sys.argv[3]
npm_hash = sys.argv[4]
text = flake_path.read_text()

def replace_exact(pattern: str, replacement: str, label: str) -> None:
    global text
    text, count = re.subn(pattern, replacement, text, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f"failed to update {label}; expected exactly one match")


replace_exact(
    r'ohMyCodexVersion = "[^"]+";',
    f'ohMyCodexVersion = "{version}";',
    "ohMyCodexVersion",
)
replace_exact(
    r'(src = pkgs\.fetchFromGitHub \{\s*owner = "Yeachan-Heo";\s*repo = "oh-my-codex";\s*rev = "v\$\{ohMyCodexVersion\}";\s*hash = )"sha256-[^"]+"(;)',
    rf'\1"{src_hash}"\2',
    "ohMyCodex src hash",
)
replace_exact(
    r'(npmDepsHash = )"sha256-[^"]+"(;)',
    rf'\1"{npm_hash}"\2',
    "ohMyCodex npmDepsHash",
)
flake_path.write_text(text)
PY

echo "updated flake.nix:"
echo "  ohMyCodexVersion = \"$version\";"
echo "  hash = \"$src_hash_sri\";"
echo "  npmDepsHash = \"$npm_deps_hash\";"
