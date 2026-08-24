#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="zeroqn"
repo="dirge"
release_tag=""

# Required release asset per supported Nix system. Keep every Linux asset
# pinned from the same release so the sandboxed dirge prebuilt is coherent.
declare -A release_assets=(
  ["x86_64-linux"]="dirge-x86_64-unknown-linux-gnu-sandbox.tar.gz"
)

usage() {
  cat <<'USAGE'
Usage: update-dirge-sandbox-prebuilt.sh [--tag <release-tag>]

Refresh the pinned dirgeSandboxPrebuiltRelease metadata in nix/pins.nix by
querying GitHub Releases and recomputing the release-asset SRI hashes from
https://github.com/zeroqn/dirge.

Default:
  --tag     newest GitHub release containing every required sandbox asset

Required release assets:
  dirge-x86_64-unknown-linux-gnu-sandbox.tar.gz
USAGE
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

required_assets_json="$(
  for required_system in "${!release_assets[@]}"; do
    printf '%s\n' "${release_assets[$required_system]}"
  done |
    jq -R . |
    jq -s .
)"

if [ -z "$release_tag" ]; then
  release_tag="$(
    curl -fsSL "https://api.github.com/repos/$owner/$repo/releases?per_page=100" |
      jq -r --argjson required_assets "$required_assets_json" '
        [
          .[]
          | select([.assets[]?.name] as $names | all($required_assets[]; . as $asset | $names | index($asset)))
        ]
        | sort_by(.published_at // .created_at)
        | last
        | .tag_name // empty
      '
  )"
fi

if [ -z "$release_tag" ]; then
  echo "failed to determine latest dirge sandbox release tag containing all required assets:" >&2
  for required_system in "${!release_assets[@]}"; do
    echo "  ${release_assets[$required_system]}" >&2
  done
  exit 1
fi

encoded_tag="$(printf '%s' "$release_tag" | jq -sRr @uri)"
release_json="$(curl -fsSL "https://api.github.com/repos/$owner/$repo/releases/tags/$encoded_tag")"

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


def nix_escape(value: str) -> str:
    # Nix double-quoted strings: escape the backslash and quote characters, and
    # prevent `${` from starting interpolation.
    return (
        value.replace("\\", "\\\\")
        .replace('"', '\\"')
        .replace("${", "\\${")
    )

assets_by_system = {
    "x86_64-linux": "dirge-x86_64-unknown-linux-gnu-sandbox.tar.gz",
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
    "  dirgeSandboxPrebuiltRelease = {",
    '    owner = "zeroqn";',
    '    repo = "dirge";',
    f'    tag = "{nix_escape(release_tag)}";',
    "    systems = {",
]

for system, asset_name in sorted(assets_by_system.items()):
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
# Use a callable replacement so re.subn does not treat backslashes in the
# replacement as a template (which would collapse "\\" and "\"" in escaped tags).
updated, count = re.subn(
    r"  dirgeSandboxPrebuiltRelease = \{.*?\n  \};",
    lambda _match: replacement,
    text,
    count=1,
    flags=re.S,
)
if count != 1:
    raise SystemExit("failed to replace dirgeSandboxPrebuiltRelease block; expected exactly one match")

pins_path.write_text(updated)
print("updated nix/pins.nix:")
print(f'  tag = "{release_tag}";')
for system, asset_name in sorted(assets_by_system.items()):
    print(f"  {system}.asset = \"{asset_name}\";")
    print(f"  {system}.hash = \"{hashes[system]}\";")
PY