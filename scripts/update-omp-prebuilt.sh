#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="can1357"
repo="oh-my-pi"
release_tag=""

usage() {
  cat <<'EOF'
Usage: update-omp-prebuilt.sh [--tag <release-tag>]

Refresh the pinned omp prebuilt release metadata in nix/pins.nix by querying
GitHub Releases and recomputing the binary SRI hashes for all supported Linux
systems.

Defaults:
  --tag     latest GitHub release
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --tag)
      release_tag="${2:?missing value for --tag}"
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

for cmd in curl jq python3; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "missing required command: $cmd" >&2
    exit 1
  fi
done

if [ -z "$release_tag" ]; then
  release_tag="$(
    curl -fsSL "https://api.github.com/repos/$owner/$repo/releases/latest" |
      jq -r '.tag_name // empty'
  )"
fi

if [ -z "$release_tag" ]; then
  echo "failed to determine latest omp release tag; pass --tag explicitly" >&2
  exit 1
fi

release_json="$(curl -fsSL "https://api.github.com/repos/$owner/$repo/releases/tags/$release_tag")"

RELEASE_JSON="$release_json" python3 - "$pins_file" "$release_tag" <<'PY'
import base64
import hashlib
import json
import os
import re
import sys
import urllib.request
from pathlib import Path

pins_path = Path(sys.argv[1])
release_tag = sys.argv[2]
release = json.loads(os.environ["RELEASE_JSON"])
text = pins_path.read_text()

assets_by_system = {
    "x86_64-linux": "omp-linux-x64",
    "aarch64-linux": "omp-linux-arm64",
}
available_assets = {
    asset["name"]: asset["browser_download_url"]
    for asset in release.get("assets", [])
}
hashes = {}

def sri_hash(url: str) -> str:
    with urllib.request.urlopen(url) as response:
        digest = hashlib.sha256(response.read()).digest()
    return "sha256-" + base64.b64encode(digest).decode()

lines = [
    "  ompPrebuiltRelease = {",
    '    owner = "can1357";',
    '    repo = "oh-my-pi";',
    f'    tag = "{release_tag}";',
    "    systems = {",
]

for system, asset_name in assets_by_system.items():
    url = available_assets.get(asset_name)
    if url is None:
        raise SystemExit(f"failed to find asset {asset_name} in release {release_tag}")
    hashes[system] = sri_hash(url)
    lines.extend([
        f"      {system} = {{",
        f'        asset = "{asset_name}";',
        f'        hash = "{hashes[system]}";',
        "      };",
    ])

lines.extend(["    };", "  };"])
replacement = "\n".join(lines)
updated, count = re.subn(
    r"  ompPrebuiltRelease = \{.*?\n  \};",
    replacement,
    text,
    count=1,
    flags=re.S,
)
if count != 1:
    raise SystemExit("failed to replace ompPrebuiltRelease block; expected exactly one match")

pins_path.write_text(updated)
print("updated nix/pins.nix:")
print(f"  tag = \"{release_tag}\";")
for system, asset_name in assets_by_system.items():
    print(f"  {system}.asset = \"{asset_name}\";")
    print(f"  {system}.hash = \"{hashes[system]}\";")
PY
