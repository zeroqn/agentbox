#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="dirge-code"
repo="dirge"
rev=""

usage() {
  cat <<'USAGE'
Usage: update-dirge.sh [--tag <release-tag>]
       update-dirge.sh [--rev <git-revision-or-release-tag>]

Refresh the pinned dirge source metadata in nix/pins.nix from
https://github.com/dirge-code/dirge. The script prefetches the source archive
and records the fixed-output source hash used to build dirge from source.

Defaults:
  --tag     latest GitHub release tag

Compatibility:
  --rev     accepted as an alias for --tag/--rev
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --tag|--rev)
      rev="${2:?missing value for $1}"
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
    curl -fsSL "https://api.github.com/repos/$owner/$repo/releases/latest" |
      jq -r '.tag_name // empty'
  )"
fi

if [ -z "$rev" ]; then
  echo "failed to determine latest dirge release; pass --tag explicitly" >&2
  exit 1
fi

archive_url="https://github.com/$owner/$repo/archive/$rev.tar.gz"
mapfile -t prefetch_output < <(nix-prefetch-url --print-path --unpack "$archive_url")
if [ "${#prefetch_output[@]}" -lt 2 ] || [ -z "${prefetch_output[0]}" ] || [ -z "${prefetch_output[1]}" ]; then
  echo "failed to prefetch dirge source archive: $archive_url" >&2
  exit 1
fi
src_hash_base32="${prefetch_output[0]}"
src_path="${prefetch_output[1]}"
src_hash_sri="$(nix hash convert --hash-algo sha256 --to sri "$src_hash_base32")"

cargo_toml="$src_path/Cargo.toml"
if [ ! -f "$cargo_toml" ]; then
  echo "failed to locate Cargo.toml in prefetched dirge source" >&2
  exit 1
fi

version="$(
  python3 - "$cargo_toml" <<'PY'
import sys
import tomllib
from pathlib import Path

data = tomllib.loads(Path(sys.argv[1]).read_text())
print(data["package"]["version"])
PY
)"

python3 - "$pins_file" "$version" "$owner" "$repo" "$rev" "$src_hash_sri" <<'PY'
import re
import sys
from pathlib import Path

pins_path = Path(sys.argv[1])
version = sys.argv[2]
owner = sys.argv[3]
repo = sys.argv[4]
rev = sys.argv[5]
src_hash = sys.argv[6]

replacement = "\n".join(
    [
        "  dirge = {",
        f'    version = "{version}";',
        f'    owner = "{owner}";',
        f'    repo = "{repo}";',
        f'    rev = "{rev}";',
        f'    srcHash = "{src_hash}";',
        "  };",
    ]
)

text = pins_path.read_text()
updated, count = re.subn(
    r"  dirge = \{\n.*?\n  \};",
    replacement,
    text,
    count=1,
    flags=re.S,
)
if count != 1:
    raise SystemExit("failed to locate dirge block in nix/pins.nix")

pins_path.write_text(updated)
PY

cat <<EOF_SUMMARY
updated nix/pins.nix:
  dirge.version = "$version";
  dirge.owner = "$owner";
  dirge.repo = "$repo";
  dirge.rev = "$rev";
  dirge.srcHash = "$src_hash_sri";
EOF_SUMMARY
