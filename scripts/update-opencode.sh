#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="anomalyco"
repo="opencode"
release_tag=""
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
Usage: update-opencode.sh [--tag <release-tag>]

Refresh the pinned OpenCode source release metadata in nix/pins.nix by querying
GitHub Releases, prefetching the source hash, and deriving the Bun node_modules
fixed-output hash. This intentionally pins anomalyco/opencode directly instead
of relying on nixpkgs' opencode package version.

Defaults:
  --tag     latest GitHub release
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

for cmd in curl jq nix nix-prefetch-url python3; do
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
  echo "failed to determine latest OpenCode release tag; pass --tag explicitly" >&2
  exit 1
fi

version="${release_tag#v}"
archive_url="https://github.com/$owner/$repo/archive/refs/tags/v$version.tar.gz"
prefetch_text="$(nix-prefetch-url --print-path --unpack "$archive_url")"
mapfile -t prefetch_output <<<"$prefetch_text"
if [ "${#prefetch_output[@]}" -lt 1 ] || [ -z "${prefetch_output[0]}" ]; then
  echo "failed to prefetch OpenCode source archive: $archive_url" >&2
  exit 1
fi
src_hash_base32="${prefetch_output[0]}"
src_hash_sri="$(nix hash convert --hash-algo sha256 --to sri "$src_hash_base32")"

python3 - "$pins_file" "$version" "$src_hash_sri" "$fake_hash" <<'PY'
import re
import sys
from pathlib import Path

pins_path = Path(sys.argv[1])
version = sys.argv[2]
src_hash = sys.argv[3]
node_modules_hash = sys.argv[4]
text = pins_path.read_text()

block_match = re.search(r'opencode = \{\n(?P<body>.*?)\n  \};', text, re.S)
if block_match is None:
    raise SystemExit("failed to locate opencode block in nix/pins.nix")

body = block_match.group("body")
updates = [
    (r'version = "[^"]+";', f'version = "{version}";', "version"),
    (r'srcHash = "sha256-[^"]+";', f'srcHash = "{src_hash}";', "srcHash"),
    (
        r'nodeModulesHash = "sha256-[^"]+";',
        f'nodeModulesHash = "{node_modules_hash}";',
        "nodeModulesHash",
    ),
]
for pattern, replacement, label in updates:
    body, count = re.subn(pattern, replacement, body, count=1)
    if count != 1:
        raise SystemExit(f"failed to update OpenCode {label} in nix/pins.nix")

pins_path.write_text(text[: block_match.start("body")] + body + text[block_match.end("body") :])
PY

set +e
build_output="$(cd "$repo_root" && nix build --no-link .#opencode 2>&1)"
build_status=$?
set -e

if [ "$build_status" -eq 0 ]; then
  echo "unexpectedly built OpenCode with the fake nodeModulesHash" >&2
  exit 1
else
  node_modules_hash="$(
    printf '%s\n' "$build_output" |
      sed -n 's/.*got:[[:space:]]*\(sha256-[^[:space:]]*\).*/\1/p' |
      tail -n 1
  )"
  if [ -z "$node_modules_hash" ]; then
    printf '%s\n' "$build_output" >&2
    echo "failed to determine OpenCode node_modules hash from nix build output" >&2
    exit "$build_status"
  fi
fi

python3 - "$pins_file" "$node_modules_hash" <<'PY'
import re
import sys
from pathlib import Path

pins_path = Path(sys.argv[1])
node_modules_hash = sys.argv[2]
text = pins_path.read_text()
block_match = re.search(r'opencode = \{\n(?P<body>.*?)\n  \};', text, re.S)
if block_match is None:
    raise SystemExit("failed to locate opencode block in nix/pins.nix")
body = block_match.group("body")
body, count = re.subn(
    r'nodeModulesHash = "sha256-[^"]+";',
    f'nodeModulesHash = "{node_modules_hash}";',
    body,
    count=1,
)
if count != 1:
    raise SystemExit("failed to update OpenCode nodeModulesHash in nix/pins.nix")
pins_path.write_text(text[: block_match.start("body")] + body + text[block_match.end("body") :])
PY

completed=1
cat <<EOF_SUMMARY
updated nix/pins.nix:
  opencode.version = "$version";
  opencode.srcHash = "$src_hash_sri";
  opencode.nodeModulesHash = "$node_modules_hash";
EOF_SUMMARY
