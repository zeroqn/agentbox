#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="Yeachan-Heo"
repo="oh-my-codex"
api_url="https://api.github.com/repos/$owner/$repo/releases/latest"
required_products=(
  omx-api
  omx-runtime
  omx-sparkshell
  omx-explore-harness
)

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
native_manifest_url="https://github.com/$owner/$repo/releases/download/v$version/native-release-manifest.json"
native_manifest_file="$(mktemp)"
trap 'rm -f "$native_manifest_file"' EXIT
curl -fsSL "$native_manifest_url" -o "$native_manifest_file"

python3 - \
  "$pins_file" \
  "$version" \
  "$src_hash_sri" \
  "$npm_deps_hash" \
  "$native_manifest_file" \
  "${required_products[@]}" \
  <<'PY'
import base64
import json
import re
import sys
from pathlib import Path

pins_path = Path(sys.argv[1])
version = sys.argv[2]
src_hash = sys.argv[3]
npm_hash = sys.argv[4]
manifest_path = Path(sys.argv[5])
required_products = sys.argv[6:]
manifest = json.loads(manifest_path.read_text())
text = pins_path.read_text()

systems = {
    "x86_64-linux": "x86_64-unknown-linux-musl",
    "aarch64-linux": "aarch64-unknown-linux-musl",
}

def sri_from_hex(hex_hash: str) -> str:
    return "sha256-" + base64.b64encode(bytes.fromhex(hex_hash)).decode("ascii")

def find_asset(product: str, target: str) -> dict:
    matches = [
        asset for asset in manifest.get("assets", [])
        if asset.get("product") == product
        and asset.get("target") == target
        and asset.get("libc") == "musl"
    ]
    if len(matches) != 1:
        raise SystemExit(f"failed to find exactly one musl asset for {product} {target}; found {len(matches)}")
    return matches[0]

lines = [
    "  ohMyCodex = {",
    f'    version = "{version}";',
    f'    srcHash = "{src_hash}";',
    f'    npmDepsHash = "{npm_hash}";',
    "    nativeBinarySystems = {",
]

for system, target in systems.items():
    lines.append(f"      {system} = {{")
    for product in required_products:
        asset = find_asset(product, target)
        lines.extend([
            f"        {product} = {{",
            f'          asset = "{asset["archive"]}";',
            f'          binary = "{asset["binary_path"]}";',
            f'          hash = "{sri_from_hex(asset["sha256"])}";',
            "        };",
        ])
    lines.append("      };")
lines.extend(["    };", "  };"])
new_block = "\n".join(lines)

updated, count = re.subn(r"  ohMyCodex = \{.*?\n  \};", new_block, text, count=1, flags=re.S)
if count != 1:
    raise SystemExit("failed to replace ohMyCodex block; expected exactly one match")
pins_path.write_text(updated)

print("updated native assets:")
for system, target in systems.items():
    print(f"  {system} ({target}):")
    for product in required_products:
        asset = find_asset(product, target)
        print(f"    {product}.asset = \"{asset['archive']}\";")
        print(f"    {product}.hash = \"{sri_from_hex(asset['sha256'])}\";")
PY

echo "updated nix/pins.nix:"
echo "  ohMyCodexVersion = \"$version\";"
echo "  hash = \"$src_hash_sri\";"
echo "  npmDepsHash = \"$npm_deps_hash\";"
