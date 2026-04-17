#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
flake_file="$repo_root/flake.nix"
owner="Yeachan-Heo"
repo="oh-my-codex"
api_url="https://api.github.com/repos/$owner/$repo/releases/latest"
placeholder_npm_deps_hash="sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

for cmd in curl jq nix-prefetch-url nix python3; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "missing required command: $cmd" >&2
    exit 1
  fi
done

extract_sri_hash_from_output() {
  local output="$1"

  printf '%s' "$output" |
    sed -E $'s/\x1B\\[[0-9;]*[[:alpha:]]//g' |
    tr '\r' '\n' |
    awk -v placeholder="$placeholder_npm_deps_hash" '
      {
        while (match($0, /sha256-[A-Za-z0-9+\/=]+/)) {
          hash = substr($0, RSTART, RLENGTH)
          if (hash != placeholder) {
            print hash
          }
          $0 = substr($0, RSTART + RLENGTH)
        }
      }
    ' |
    tail -n 1
}

release_json="$(curl -fsSL "$api_url")"
version="$(printf '%s' "$release_json" | jq -r '.tag_name' | sed 's/^v//')"

if [ -z "$version" ] || [ "$version" = "null" ]; then
  echo "failed to determine latest oh-my-codex release tag" >&2
  exit 1
fi

archive_url="https://github.com/$owner/$repo/archive/refs/tags/v$version.tar.gz"
src_hash_base32="$(nix-prefetch-url --unpack "$archive_url")"
src_hash_sri="$(nix hash convert --hash-algo sha256 --to sri "$src_hash_base32")"

tmpdir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

tmp_flake="$tmpdir/flake.nix"
cp "$flake_file" "$tmp_flake"
if [ -f "$repo_root/flake.lock" ]; then
  cp "$repo_root/flake.lock" "$tmpdir/flake.lock"
fi

python3 - "$tmp_flake" "$version" "$src_hash_sri" "$placeholder_npm_deps_hash" <<'PY'
import re
import sys
from pathlib import Path

flake_path = Path(sys.argv[1])
version = sys.argv[2]
src_hash = sys.argv[3]
text = flake_path.read_text()
text = re.sub(r'ohMyCodexVersion = "[^"]+";', f'ohMyCodexVersion = "{version}";', text)
text = re.sub(r'hash = "sha256-[^"]+";', f'hash = "{src_hash}";', text, count=1)
text = re.sub(
    r'npmDepsHash = "sha256-[^"]+";',
    f'npmDepsHash = "{sys.argv[4]}";',
    text,
    count=1,
)
flake_path.write_text(text)
PY

set +e
build_output="$(
  nix build "path:$tmpdir#oh-my-codex" 2>&1
)"
build_status=$?
set -e

if [ "$build_status" -eq 0 ]; then
  echo "unexpectedly resolved npmDepsHash without a mismatch; inspect the package manually" >&2
  exit 1
fi

npm_deps_hash="$(extract_sri_hash_from_output "$build_output")"

if [ -z "$npm_deps_hash" ]; then
  echo "failed to extract npmDepsHash from nix build output" >&2
  printf '%s\n' "$build_output" >&2
  exit 1
fi

python3 - "$flake_file" "$version" "$src_hash_sri" "$npm_deps_hash" <<'PY'
import re
import sys
from pathlib import Path

flake_path = Path(sys.argv[1])
version = sys.argv[2]
src_hash = sys.argv[3]
npm_hash = sys.argv[4]
text = flake_path.read_text()
text = re.sub(r'ohMyCodexVersion = "[^"]+";', f'ohMyCodexVersion = "{version}";', text)
text = re.sub(r'hash = "sha256-[^"]+";', f'hash = "{src_hash}";', text, count=1)
text = re.sub(r'npmDepsHash = "sha256-[^"]+";', f'npmDepsHash = "{npm_hash}";', text, count=1)
flake_path.write_text(text)
PY

echo "updated flake.nix:"
echo "  ohMyCodexVersion = \"$version\";"
echo "  hash = \"$src_hash_sri\";"
echo "  npmDepsHash = \"$npm_deps_hash\";"
