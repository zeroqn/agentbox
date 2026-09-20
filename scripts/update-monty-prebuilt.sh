#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
npm_package="@pydantic/monty-linux-x64-gnu"
asset_base="monty-linux-x64-gnu"
version=""

usage() {
  cat <<'EOF'
Usage: update-monty-prebuilt.sh [--version <x.y.z>]

Refresh the pinned monty prebuilt worker metadata in nix/pins.nix from the
published @pydantic/monty-linux-x64-gnu npm tarball.

Keep the version in sync with the @pydantic/monty JS client the RLM extension
installs: client and worker reject each other over a protocol-version mismatch.

Defaults:
  --version   latest version published to the npm registry
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      version="${2:?missing value for --version}"
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

if [ -z "$version" ]; then
  version="$(
    curl -fsSL "https://registry.npmjs.org/$npm_package/latest" |
      jq -r '.version // empty'
  )"
fi

if [ -z "$version" ]; then
  echo "failed to determine the latest $npm_package version; pass --version explicitly" >&2
  exit 1
fi

asset="$asset_base-$version.tgz"
release_url="https://registry.npmjs.org/$npm_package/-/$asset"

python3 - "$pins_file" "$version" "$asset" "$release_url" <<'PY'
import base64
import hashlib
import re
import sys
import urllib.request
from pathlib import Path

pins_path = Path(sys.argv[1])
version, asset, release_url = sys.argv[2], sys.argv[3], sys.argv[4]

with urllib.request.urlopen(release_url) as response:
    digest = hashlib.sha256(response.read()).digest()
sri_hash = "sha256-" + base64.b64encode(digest).decode()

replacement = "\n".join([
    "  montyPrebuiltRelease = {",
    f'    version = "{version}";',
    "    systems = {",
    "      x86_64-linux = {",
    f'        asset = "{asset}";',
    f'        hash = "{sri_hash}";',
    "      };",
    "    };",
    "  };",
])
text = pins_path.read_text()
updated, count = re.subn(
    r"  montyPrebuiltRelease = \{.*?\n  \};",
    replacement,
    text,
    count=1,
    flags=re.S,
)
if count != 1:
    raise SystemExit("failed to replace montyPrebuiltRelease block; expected exactly one match")

pins_path.write_text(updated)
print("updated nix/pins.nix:")
print(f'  version = "{version}";')
print(f'  x86_64-linux.asset = "{asset}";')
print(f'  x86_64-linux.hash = "{sri_hash}";')
PY
