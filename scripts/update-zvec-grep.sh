#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="zvec-ai"
repo="zvec-grep"
release_tag=""
rev=""
work_dir="$(mktemp -d)"
completed=0
original_pins="$(mktemp)"
cp "$pins_file" "$original_pins"

cleanup() {
  if [ "$completed" -ne 1 ]; then
    cp "$original_pins" "$pins_file"
  fi
  rm -f "$original_pins"
  rm -rf "$work_dir"
}
trap cleanup EXIT

usage() {
  cat <<'USAGE'
Usage: update-zvec-grep.sh [--tag <release-tag>] [--rev <git-revision>]

Refresh the pinned zvec-grep source and npm dependency hashes in nix/pins.nix by
fetching the GitHub source archive and prefetching the upstream npm lockfile.

Defaults:
  --tag  latest GitHub release tag
  --rev  same as --tag
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

for cmd in curl jq python3 nix nix-prefetch-url; do
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
  echo "failed to determine latest zvec-grep release tag; pass --tag explicitly" >&2
  exit 1
fi

if [ -z "$rev" ]; then
  rev="$release_tag"
fi

archive_url="https://github.com/$owner/$repo/archive/$rev.tar.gz"
mapfile -t prefetch_output < <(nix-prefetch-url --print-path --unpack "$archive_url")
if [ "${#prefetch_output[@]}" -lt 2 ] || [ -z "${prefetch_output[0]}" ] || [ -z "${prefetch_output[1]}" ]; then
  echo "failed to prefetch zvec-grep source archive: $archive_url" >&2
  exit 1
fi
src_hash_base32="${prefetch_output[0]}"
src_path="${prefetch_output[1]}"
src_hash_sri="$(nix hash convert --hash-algo sha256 --to sri "$src_hash_base32")"

source_copy="$work_dir/source"
cp -R "$src_path" "$source_copy"
chmod -R u+w "$source_copy"

package_json="$source_copy/package.json"
lockfile="$source_copy/package-lock.json"
if [ ! -f "$package_json" ]; then
  echo "failed to locate package.json in unpacked zvec-grep source" >&2
  exit 1
fi
if [ ! -f "$lockfile" ]; then
  echo "failed to locate package-lock.json in unpacked zvec-grep source" >&2
  exit 1
fi

version="$(jq -r '.version // empty' "$package_json")"
if [ -z "$version" ]; then
  echo "failed to read zvec-grep version from $package_json" >&2
  exit 1
fi

bin_target="$(jq -r '.bin.zg // empty' "$package_json")"
if [ "$bin_target" != "dist/cli/index.js" ]; then
  echo "zvec-grep package.json bin.zg is '$bin_target', expected 'dist/cli/index.js'" >&2
  echo "update nix/pkgs/zvec-grep.nix installPhase before pinning this release" >&2
  exit 1
fi

prefetch_npm_deps="$(nix build --no-link --print-out-paths nixpkgs#prefetch-npm-deps)/bin/prefetch-npm-deps"
npm_cache="$work_dir/npm-cache"
for attempt in 1 2 3; do
  rm -rf "$npm_cache"
  if NPM_FETCHER_VERSION=2 "$prefetch_npm_deps" "$lockfile" "$npm_cache"; then
    break
  fi
  if [ "$attempt" -eq 3 ]; then
    echo "failed to prefetch zvec-grep npm dependencies after $attempt attempts" >&2
    exit 1
  fi
  echo "npm dependency prefetch failed; retrying ($attempt/3)" >&2
  sleep 2
done

npm_deps_hash="$(nix hash path "$npm_cache")"
nix store add-path --name "zvec-grep-$version-npm-deps" "$npm_cache" >/dev/null

python3 - "$pins_file" "$version" "$owner" "$repo" "$rev" "$src_hash_sri" "$npm_deps_hash" <<'PY'
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
        "  zvecGrep = {",
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
    r"  zvecGrep = \{\n.*?\n  \};",
    replacement,
    text,
    count=1,
    flags=re.S,
)
if count != 1:
    raise SystemExit("failed to locate zvecGrep block in nix/pins.nix")

pins_path.write_text(updated)
PY

completed=1
cat <<EOF_SUMMARY
updated nix/pins.nix:
  zvecGrep.version = "$version";
  zvecGrep.owner = "$owner";
  zvecGrep.repo = "$repo";
  zvecGrep.rev = "$rev";
  zvecGrep.srcHash = "$src_hash_sri";
  zvecGrep.npmDepsHash = "$npm_deps_hash"
EOF_SUMMARY
