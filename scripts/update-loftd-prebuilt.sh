#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="zeroqn"
repo="agentbox"
system="x86_64-linux"
release_tag=""

usage() {
  cat <<'USAGE'
Usage: update-loftd-prebuilt.sh [--tag <release-tag>] [--system <system>]

Refresh the pinned loftd prebuilt release metadata in nix/pins.nix by querying
GitHub Releases, rejecting legacy/concrete-store-referencing payloads, and
recomputing the binary SRI hash.

Defaults:
  --tag     newest sha-* prerelease containing the selected loftd asset
  --system  x86_64-linux
USAGE
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
    asset_name="loftd-x86_64-unknown-linux-gnu"
    ;;
  aarch64-linux)
    asset_name="loftd-aarch64-unknown-linux-gnu"
    ;;
  *)
    echo "unsupported system: $system" >&2
    exit 1
    ;;
esac

if [[ "$asset_name" == *-linux-flake-locked ]]; then
  echo "internal error: refusing legacy loftd flake-locked asset name: $asset_name" >&2
  exit 1
fi

releases_api="https://api.github.com/repos/$owner/$repo/releases?per_page=100"

if [ -z "$release_tag" ]; then
  release_tag="$(
    curl -fsSL "$releases_api" |
      jq -r --arg asset_name "$asset_name" '
        map(
          select(.tag_name | startswith("sha-"))
          | select(any(.assets[]?; .name == $asset_name))
        )
        | sort_by(.published_at // .created_at)
        | reverse
        | .[0].tag_name // empty
      '
  )"
fi

if [ -z "$release_tag" ]; then
  echo "failed to determine a sha-* release tag containing $asset_name; pass --tag explicitly after publishing one" >&2
  exit 1
fi

download_url="https://github.com/$owner/$repo/releases/download/$release_tag/$asset_name"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
asset_path="$tmp_dir/$asset_name"

python3 - "$download_url" "$asset_path" "$asset_name" "$release_tag" <<'PY'
import pathlib
import sys
import urllib.request

url = sys.argv[1]
path = pathlib.Path(sys.argv[2])
asset_name = sys.argv[3]
release_tag = sys.argv[4]
try:
    with urllib.request.urlopen(url) as response:
        path.write_bytes(response.read())
except Exception as error:
    raise SystemExit(f"failed to download {asset_name} from {release_tag}: {error}") from error
PY

python3 - "$asset_path" "$asset_name" "$release_tag" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
asset_name = sys.argv[2]
release_tag = sys.argv[3]
data = path.read_bytes()
if data.startswith(b"#!"):
    raise SystemExit(
        f"upstream asset blocker: {asset_name} in {release_tag} is a wrapper script, not raw ELF"
    )
if data[:4] != b"\x7fELF":
    raise SystemExit(
        f"upstream asset blocker: {asset_name} in {release_tag} is not an ELF payload"
    )
if re.search(rb"/nix/store/[0-9a-df-np-sv-z]{32}-", data):
    raise SystemExit(
        f"upstream asset blocker: {asset_name} in {release_tag} contains concrete /nix/store references; publish a neutral loftd asset"
    )
PY

asset_hash="$(
  python3 - "$asset_path" <<'PY'
import base64
import hashlib
import pathlib
import sys

digest = hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).digest()
print("sha256-" + base64.b64encode(digest).decode())
PY
)"

python3 - "$pins_file" "$release_tag" "$system" "$asset_name" "$asset_hash" <<'PY'
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
    r'loftdPrebuiltRelease = \{\n(?P<body>.*?)\n  \};',
    text,
    re.S,
)
if block_match is None:
    raise SystemExit("failed to locate loftdPrebuiltRelease block in nix/pins.nix")

body = block_match.group("body")
body, tag_count = re.subn(r'tag = "[^"]+";', f'tag = "{release_tag}";', body, count=1)
if tag_count != 1:
    raise SystemExit("failed to update loftd prebuilt release tag in nix/pins.nix")

system_entry = (
    f'      {system} = {{\n'
    f'        asset = "{asset_name}";\n'
    f'        hash = "{asset_hash}";\n'
    f'      }};'
)

system_pattern = re.compile(
    rf'(      {re.escape(system)} = \{{\n\s+asset = ")[^"]+(";\n\s+hash = ")[^"]+(";\n\s+\}};)',
    re.S,
)
body, system_count = system_pattern.subn(rf'\1{asset_name}\2{asset_hash}\3', body, count=1)

if system_count == 0:
    empty_systems = 'systems = { };'
    if empty_systems in body:
        body = body.replace(empty_systems, f'systems = {{\n{system_entry}\n    }};', 1)
    else:
        systems_match = re.search(r'(systems = \{\n)(?P<systems>.*?)(    \};)', body, re.S)
        if systems_match is None:
            raise SystemExit("failed to locate loftdPrebuiltRelease.systems block in nix/pins.nix")
        existing = systems_match.group("systems")
        insertion = existing
        if insertion and not insertion.endswith("\n"):
            insertion += "\n"
        insertion += system_entry + "\n"
        body = body[: systems_match.start("systems")] + insertion + body[systems_match.end("systems") :]

updated = text[: block_match.start("body")] + body + text[block_match.end("body") :]
pins_path.write_text(updated)
PY

cat <<EOF_OUT
updated nix/pins.nix:
  tag = "$release_tag";
  $system.asset = "$asset_name";
  $system.hash = "$asset_hash";
EOF_OUT
