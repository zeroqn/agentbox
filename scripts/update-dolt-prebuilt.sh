#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="dolthub"
repo="dolt"
release_tag=""

usage() {
  cat <<'EOF'
Usage: update-dolt-prebuilt.sh [--tag <release-tag>]

Refresh the pinned Dolt prebuilt release metadata in nix/pins.nix by querying
GitHub Releases and recomputing the release-asset SRI hashes for all supported
Linux systems.

Defaults:
  --tag     latest GitHub release containing every supported Linux tarball
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
    curl -fsSL "https://api.github.com/repos/$owner/$repo/releases?per_page=100" |
      jq -r '
        [
          .[]
          | select(any(.assets[]?; .name == "dolt-linux-amd64.tar.gz"))
          | select(any(.assets[]?; .name == "dolt-linux-arm64.tar.gz"))
        ]
        | sort_by(.published_at // .created_at)
        | reverse
        | .[0].tag_name // empty
      '
  )"
fi

if [ -z "$release_tag" ]; then
  echo "failed to determine latest Dolt release; pass --tag explicitly" >&2
  exit 1
fi

release_json="$(curl -fsSL "https://api.github.com/repos/$owner/$repo/releases/tags/$release_tag")"

# Pass the release payload through a file: a large release body can exceed the
# per-string argv/env limit (E2BIG) when exported as an environment variable.
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
release_json_file="$tmpdir/release.json"
printf '%s' "$release_json" > "$release_json_file"

python3 - "$pins_file" "$release_tag" "$release_json_file" <<'PY'
import base64
import hashlib
import json
import re
import sys
import urllib.request
from pathlib import Path

pins_path = Path(sys.argv[1])
release_tag = sys.argv[2]
release = json.loads(Path(sys.argv[3]).read_text())
text = pins_path.read_text()

assets_by_system = {
    "x86_64-linux": "dolt-linux-amd64.tar.gz",
    "aarch64-linux": "dolt-linux-arm64.tar.gz",
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
    "  doltPrebuiltRelease = {",
    '    owner = "dolthub";',
    '    repo = "dolt";',
    f'    tag = "{release_tag}";',
    "    systems = {",
]

for system, asset_name in assets_by_system.items():
    url = available_assets.get(asset_name)
    if url is None:
        raise SystemExit(f"failed to find asset {asset_name} in release {release_tag}")
    hashes[system] = sri_hash(url)
    lines.extend(
        [
            f"      {system} = {{",
            f'        asset = "{asset_name}";',
            f'        hash = "{hashes[system]}";',
            "      };",
        ]
    )

lines.extend(["    };", "  };"])
replacement = "\n".join(lines)
updated, count = re.subn(
    r"  doltPrebuiltRelease = \{.*?\n  \};",
    replacement,
    text,
    count=1,
    flags=re.S,
)
if count != 1:
    raise SystemExit("failed to replace doltPrebuiltRelease block; expected exactly one match")

pins_path.write_text(updated)
print("updated nix/pins.nix:")
print(f'  tag = "{release_tag}";')
for system, asset_name in assets_by_system.items():
    print(f'  {system}.asset = "{asset_name}";')
    print(f'  {system}.hash = "{hashes[system]}";')
PY
