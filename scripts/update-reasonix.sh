#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="esengine"
repo="DeepSeek-Reasonix"
release_tag=""
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
Usage: update-reasonix.sh [--tag <release-tag>] [--rev <git-revision>]

Refresh the pinned Reasonix source/npm metadata in nix/pins.nix from
https://github.com/esengine/DeepSeek-Reasonix. By default, the script uses the
latest GitHub release and pins the release target commit instead of a desktop
installer asset.

Defaults:
  --tag     latest GitHub release tag
  --rev     release target commit for --tag
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --tag)
      release_tag="${2:?missing value for --tag}"
      shift 2
      ;;
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

release_json=""
if [ -z "$release_tag" ] && [ -z "$rev" ]; then
  release_json="$(curl -fsSL "https://api.github.com/repos/$owner/$repo/releases/latest")"
  release_tag="$(jq -r '.tag_name // empty' <<<"$release_json")"
elif [ -n "$release_tag" ]; then
  release_json="$(curl -fsSL "https://api.github.com/repos/$owner/$repo/releases/tags/$release_tag")"
fi

if [ -z "$rev" ] && [ -n "$release_json" ]; then
  rev="$(jq -r '.target_commitish // empty' <<<"$release_json")"
fi

if [ -z "$rev" ]; then
  echo "failed to determine Reasonix release target rev; pass --rev explicitly" >&2
  exit 1
fi

archive_url="https://github.com/$owner/$repo/archive/$rev.tar.gz"
mapfile -t prefetch_output < <(nix-prefetch-url --print-path --unpack "$archive_url")
if [ "${#prefetch_output[@]}" -lt 2 ] || [ -z "${prefetch_output[0]}" ] || [ -z "${prefetch_output[1]}" ]; then
  echo "failed to prefetch Reasonix source archive: $archive_url" >&2
  exit 1
fi
src_hash_base32="${prefetch_output[0]}"
src_path="${prefetch_output[1]}"
src_hash_sri="$(nix hash convert --hash-algo sha256 --to sri "$src_hash_base32")"

package_json="$src_path/package.json"
if [ ! -f "$package_json" ]; then
  echo "failed to locate package.json in unpacked Reasonix source" >&2
  exit 1
fi
version="$(jq -r '.version // empty' "$package_json")"
if [ -z "$version" ]; then
  echo "failed to read Reasonix version from $package_json" >&2
  exit 1
fi

python3 - "$pins_file" "$version" "$owner" "$repo" "$rev" "$src_hash_sri" "$fake_hash" <<'PY'
import re
import sys
from pathlib import Path

pins_path = Path(sys.argv[1])
version = sys.argv[2]
owner = sys.argv[3]
repo = sys.argv[4]
rev = sys.argv[5]
src_hash = sys.argv[6]
npm_deps_hash = sys.argv[7]
replacement = "\n".join(
    [
        "  reasonix = {",
        f'    version = "{version}";',
        f'    owner = "{owner}";',
        f'    repo = "{repo}";',
        f'    rev = "{rev}";',
        f'    srcHash = "{src_hash}";',
        f'    npmDepsHash = "{npm_deps_hash}";',
        "  };",
    ]
)
text = pins_path.read_text()
updated, count = re.subn(
    r"  reasonix = \{\n.*?\n  \};",
    replacement,
    text,
    count=1,
    flags=re.S,
)
if count != 1:
    raise SystemExit("failed to locate reasonix block in nix/pins.nix")
pins_path.write_text(updated)
PY

set +e
build_output="$(cd "$repo_root" && nix build --no-link .#reasonix 2>&1)"
build_status=$?
set -e

if [ "$build_status" -eq 0 ]; then
  echo "unexpectedly built Reasonix with the fake npmDepsHash" >&2
  exit 1
fi

npm_deps_hash="$(
  printf '%s\n' "$build_output" |
    sed -n 's/.*got:[[:space:]]*\(sha256-[^[:space:]]*\).*/\1/p' |
    tail -n 1
)"
if [ -z "$npm_deps_hash" ]; then
  printf '%s\n' "$build_output" >&2
  echo "failed to determine Reasonix npm dependency hash from nix build output" >&2
  exit "$build_status"
fi

python3 - "$pins_file" "$npm_deps_hash" <<'PY'
import re
import sys
from pathlib import Path

pins_path = Path(sys.argv[1])
npm_deps_hash = sys.argv[2]
text = pins_path.read_text()
block_match = re.search(r"reasonix = \{\n(?P<body>.*?)\n  \};", text, re.S)
if block_match is None:
    raise SystemExit("failed to locate reasonix block in nix/pins.nix")
body = block_match.group("body")
body, count = re.subn(
    r'npmDepsHash = "sha256-[^"]+";',
    f'npmDepsHash = "{npm_deps_hash}";',
    body,
    count=1,
)
if count != 1:
    raise SystemExit("failed to update Reasonix npmDepsHash in nix/pins.nix")
pins_path.write_text(text[: block_match.start("body")] + body + text[block_match.end("body") :])
PY

completed=1
cat <<EOF_SUMMARY
updated nix/pins.nix:
  reasonix.version = "$version";
  reasonix.rev = "$rev";
  reasonix.srcHash = "$src_hash_sri";
  reasonix.npmDepsHash = "$npm_deps_hash";
EOF_SUMMARY
