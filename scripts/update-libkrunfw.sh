#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="zeroqn"
repo="libkrunfw"
system="x86_64-linux"
release_tag=""

usage() {
  cat <<'USAGE_EOF'
Usage: update-libkrunfw.sh [--tag <release-tag>] [--system <system>]

Refresh the pinned zeroqn/libkrunfw release metadata in nix/pins.nix by querying
GitHub Releases and recomputing the selected release-asset SRI hash.

Defaults:
  --tag     latest GitHub release
  --system  x86_64-linux

Supported systems:
  x86_64-linux, aarch64-linux, riscv64-linux
USAGE_EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --tag)
      release_tag="${2:?missing value for --tag}"
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

for cmd in curl jq python3; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "missing required command: $cmd" >&2
    exit 1
  fi
done

case "$system" in
  x86_64-linux)
    asset_name="libkrunfw-x86_64.tgz"
    ;;
  aarch64-linux)
    asset_name="libkrunfw-aarch64.tgz"
    ;;
  riscv64-linux)
    asset_name="libkrunfw-riscv64.tgz"
    ;;
  *)
    echo "unsupported system: $system" >&2
    exit 1
    ;;
esac

if [ -z "$release_tag" ]; then
  release_tag="$(
    curl -fsSL "https://api.github.com/repos/$owner/$repo/releases/latest" |
      jq -r '.tag_name // empty'
  )"
fi

if [ -z "$release_tag" ]; then
  echo "failed to determine latest libkrunfw release tag; pass --tag explicitly" >&2
  exit 1
fi

release_api="https://api.github.com/repos/$owner/$repo/releases/tags/$release_tag"
release_json="$(curl -fsSL "$release_api")"
download_url="$(
  printf '%s' "$release_json" |
    jq -r --arg asset_name "$asset_name" '
      .assets[]
      | select(.name == $asset_name)
      | .browser_download_url
    ' |
    head -n 1
)"

if [ -z "$download_url" ] || [ "$download_url" = "null" ]; then
  echo "failed to find asset $asset_name in release $release_tag" >&2
  exit 1
fi

asset_hash="$(
  python3 - "$download_url" <<'PY_EOF'
import base64
import hashlib
import sys
import urllib.request

url = sys.argv[1]
with urllib.request.urlopen(url) as response:
    digest = hashlib.sha256(response.read()).digest()
print("sha256-" + base64.b64encode(digest).decode())
PY_EOF
)"

python3 - "$pins_file" "$release_tag" "$system" "$asset_name" "$asset_hash" <<'PY_EOF'
import re
import sys
from pathlib import Path

pins_path = Path(sys.argv[1])
release_tag = sys.argv[2]
system = sys.argv[3]
asset_name = sys.argv[4]
asset_hash = sys.argv[5]
text = pins_path.read_text()

block_match = re.search(
    r'libkrunfwRelease = \{\n(?P<body>.*?)\n  \};',
    text,
    re.S,
)
if block_match is None:
    raise SystemExit("failed to locate libkrunfwRelease block in nix/pins.nix")

body = block_match.group("body")
body, tag_count = re.subn(r'tag = "[^"]+";', f'tag = "{release_tag}";', body, count=1)
if tag_count != 1:
    raise SystemExit("failed to update libkrunfw release tag in nix/pins.nix")

system_pattern = re.compile(
    rf'({re.escape(system)} = \{{\n\s+asset = ")[^"]+(";\n\s+hash = ")[^"]+(";)',
    re.S,
)
body, system_count = system_pattern.subn(rf'\1{asset_name}\2{asset_hash}\3', body, count=1)
if system_count != 1:
    raise SystemExit(f"failed to update libkrunfw asset metadata for {system} in nix/pins.nix")

updated = text[: block_match.start("body")] + body + text[block_match.end("body") :]
pins_path.write_text(updated)
PY_EOF

cat <<REPORT_EOF
updated nix/pins.nix:
  tag = "$release_tag";
  $system.asset = "$asset_name";
  $system.hash = "$asset_hash";
REPORT_EOF
