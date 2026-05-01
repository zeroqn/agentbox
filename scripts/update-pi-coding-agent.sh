#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="badlogic"
repo="pi-mono"
rev=""
fake_hash="sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
completed=0
original_pins="$(mktemp)"
cp "$pins_file" "$original_pins"
cleanup() {
  if [ "$completed" -ne 1 ]; then
    cp "$original_pins" "$pins_file"
  fi
  rm -f "$original_pins"
}
trap cleanup EXIT

usage() {
  cat <<'USAGE'
Usage: update-pi-coding-agent.sh [--rev <git-revision>]

Refresh the pinned Pi coding agent metadata in nix/pins.nix from
badlogic/pi-mono/packages/coding-agent. The script prefetches the monorepo
source archive, reads the coding-agent package version, and derives the npm
fixed-output dependency hash by building .#pi-coding-agent with a fake hash.

Defaults:
  --rev     latest commit on GitHub's main branch
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --rev)
      rev="${2:?missing value for --rev}"
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
    curl -fsSL "https://api.github.com/repos/$owner/$repo/commits/main" |
      jq -r '.sha // empty'
  )"
fi

if [ -z "$rev" ]; then
  echo "failed to determine latest pi-mono main revision; pass --rev explicitly" >&2
  exit 1
fi

archive_url="https://github.com/$owner/$repo/archive/$rev.tar.gz"
mapfile -t prefetch_output < <(nix-prefetch-url --print-path --unpack "$archive_url")
if [ "${#prefetch_output[@]}" -lt 2 ] || [ -z "${prefetch_output[0]}" ] || [ -z "${prefetch_output[1]}" ]; then
  echo "failed to prefetch Pi source archive: $archive_url" >&2
  exit 1
fi
src_hash_base32="${prefetch_output[0]}"
src_path="${prefetch_output[1]}"
src_hash_sri="$(nix hash convert --hash-algo sha256 --to sri "$src_hash_base32")"

package_json="$src_path/packages/coding-agent/package.json"
if [ ! -f "$package_json" ]; then
  echo "failed to locate packages/coding-agent/package.json in unpacked source" >&2
  exit 1
fi
version="$(jq -r '.version // empty' "$package_json")"
if [ -z "$version" ]; then
  echo "failed to read Pi coding agent version from $package_json" >&2
  exit 1
fi

python3 - "$pins_file" "$version" "$rev" "$src_hash_sri" "$fake_hash" <<'PY'
import re
import sys
from pathlib import Path

pins_path = Path(sys.argv[1])
version = sys.argv[2]
rev = sys.argv[3]
src_hash = sys.argv[4]
npm_deps_hash = sys.argv[5]
text = pins_path.read_text()

block_match = re.search(r'piCodingAgent = \{\n(?P<body>.*?)\n  \};', text, re.S)
if block_match is None:
    raise SystemExit("failed to locate piCodingAgent block in nix/pins.nix")

body = block_match.group("body")
updates = [
    (r'version = "[^"]+";', f'version = "{version}";', "version"),
    (r'rev = "[^"]+";', f'rev = "{rev}";', "rev"),
    (r'srcHash = "sha256-[^"]+";', f'srcHash = "{src_hash}";', "srcHash"),
    (
        r'npmDepsHash = "sha256-[^"]+";',
        f'npmDepsHash = "{npm_deps_hash}";',
        "npmDepsHash",
    ),
]
for pattern, replacement, label in updates:
    body, count = re.subn(pattern, replacement, body, count=1)
    if count != 1:
        raise SystemExit(f"failed to update Pi coding agent {label} in nix/pins.nix")

pins_path.write_text(text[: block_match.start("body")] + body + text[block_match.end("body") :])
PY

set +e
build_output="$(cd "$repo_root" && nix build --cores 1 --no-link .#pi-coding-agent 2>&1)"
build_status=$?
set -e

if [ "$build_status" -eq 0 ]; then
  echo "unexpectedly built Pi coding agent with the fake npmDepsHash" >&2
  exit 1
else
  npm_deps_hash="$(
    printf '%s\n' "$build_output" |
      sed -n 's/.*got:[[:space:]]*\(sha256-[^[:space:]]*\).*/\1/p' |
      tail -n 1
  )"
  if [ -z "$npm_deps_hash" ]; then
    printf '%s\n' "$build_output" >&2
    echo "failed to determine Pi coding agent npm deps hash from nix build output" >&2
    exit "$build_status"
  fi
fi

python3 - "$pins_file" "$npm_deps_hash" <<'PY'
import re
import sys
from pathlib import Path

pins_path = Path(sys.argv[1])
npm_deps_hash = sys.argv[2]
text = pins_path.read_text()
block_match = re.search(r'piCodingAgent = \{\n(?P<body>.*?)\n  \};', text, re.S)
if block_match is None:
    raise SystemExit("failed to locate piCodingAgent block in nix/pins.nix")
body = block_match.group("body")
body, count = re.subn(
    r'npmDepsHash = "sha256-[^"]+";',
    f'npmDepsHash = "{npm_deps_hash}";',
    body,
    count=1,
)
if count != 1:
    raise SystemExit("failed to update Pi coding agent npmDepsHash in nix/pins.nix")
pins_path.write_text(text[: block_match.start("body")] + body + text[block_match.end("body") :])
PY

completed=1
cat <<EOF_SUMMARY
updated nix/pins.nix:
  piCodingAgent.version = "$version";
  piCodingAgent.rev = "$rev";
  piCodingAgent.srcHash = "$src_hash_sri";
  piCodingAgent.npmDepsHash = "$npm_deps_hash";
EOF_SUMMARY
