#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins_file="$repo_root/nix/pins.nix"
owner="zeroqn"
repo="libkrun"
ref="loftd"
rev=""
system="${UPDATE_LIBKRUN_SYSTEM:-x86_64-linux}"
mode="source"
release_prefix="loftd-"
required_release_systems=("x86_64-linux" "aarch64-linux")

declare -A release_assets=(
  ["x86_64-linux"]="libkrun-x86_64-linux-full.tgz"
  ["aarch64-linux"]="libkrun-aarch64-linux-full.tgz"
)

usage() {
  cat <<'USAGE_EOF'
Usage: update-libkrun.sh [--rev <revision>] [--ref <ref>] [--system <system>]
       update-libkrun.sh --prebuilt-release

Refresh libkrun metadata in nix/pins.nix.

Default source mode:
  --ref     loftd
  --system  current updater system, default x86_64-linux

  Recomputes both the GitHub source SRI hash and the Cargo vendor SRI hash used
  by nix/pkgs/libkrun.nix.

Prebuilt release mode:
  --prebuilt-release

  Selects the newest zeroqn/libkrun prerelease tag matching loftd-* that contains
  every required first-pass Linux asset, recomputes all asset hashes from that
  single tag, writes libkrunRelease.*, and sets libkrunRelease.enabled = true.
USAGE_EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --prebuilt-release)
      mode="prebuilt-release"
      shift
      ;;
    --source)
      mode="source"
      shift
      ;;
    --rev)
      rev="${2:?missing value for --rev}"
      shift 2
      ;;
    --ref)
      ref="${2:?missing value for --ref}"
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

require_commands() {
  for cmd in "$@"; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      echo "missing required command: $cmd" >&2
      exit 1
    fi
  done
}

hash_url() {
  python3 - "$1" <<'PY_EOF'
import base64
import hashlib
import sys
import urllib.request

url = sys.argv[1]
with urllib.request.urlopen(url) as response:
    digest = hashlib.sha256(response.read()).digest()
print("sha256-" + base64.b64encode(digest).decode())
PY_EOF
}

update_source_pin() {
  require_commands curl jq nix nix-prefetch-url python3

  if [ -z "$rev" ]; then
    rev="$(
      curl -fsSL "https://api.github.com/repos/$owner/$repo/commits/$ref" |
        jq -r '.sha // empty'
    )"
  fi

  if [ -z "$rev" ] || [ "$rev" = "null" ]; then
    echo "failed to resolve libkrun revision for ref: $ref" >&2
    exit 1
  fi

  nixpkgs_rev="$(jq -r '.nodes.nixpkgs.locked.rev' "$repo_root/flake.lock")"
  if [ -z "$nixpkgs_rev" ] || [ "$nixpkgs_rev" = "null" ]; then
    echo "failed to read nixpkgs revision from flake.lock" >&2
    exit 1
  fi

  src_url="https://github.com/$owner/$repo/archive/$rev.tar.gz"
  src_hash_nix32="$(nix-prefetch-url --unpack "$src_url")"
  src_hash="$(nix hash convert --hash-algo sha256 --from nix32 --to sri "$src_hash_nix32")"

  expr="
let
  pkgs = (builtins.getFlake \"github:NixOS/nixpkgs/$nixpkgs_rev\").legacyPackages.$system;
  src = pkgs.fetchFromGitHub {
    owner = \"$owner\";
    repo = \"$repo\";
    rev = \"$rev\";
    hash = \"$src_hash\";
  };
in pkgs.rustPlatform.fetchCargoVendor {
  inherit src;
  hash = pkgs.lib.fakeHash;
}
"

  set +e
  cargo_output="$(
    nix build --no-link --extra-experimental-features 'nix-command flakes' --expr "$expr" 2>&1
  )"
  cargo_status=$?
  set -e

  if [ "$cargo_status" -eq 0 ]; then
    echo "unexpectedly built Cargo vendor derivation with a fake hash" >&2
    exit 1
  fi

  cargo_deps_hash="$(printf '%s\n' "$cargo_output" | sed -n 's/^[[:space:]]*got:[[:space:]]*\(sha256-[^[:space:]]*\)$/\1/p' | tail -n 1)"
  if [ -z "$cargo_deps_hash" ]; then
    printf '%s\n' "$cargo_output" >&2
    echo "failed to extract Cargo vendor hash" >&2
    exit 1
  fi

  python3 - "$pins_file" "$owner" "$repo" "$rev" "$src_hash" "$cargo_deps_hash" <<'PY_EOF'
import re
import sys
from pathlib import Path

pins_path = Path(sys.argv[1])
owner = sys.argv[2]
repo = sys.argv[3]
rev = sys.argv[4]
src_hash = sys.argv[5]
cargo_deps_hash = sys.argv[6]
text = pins_path.read_text()

block_match = re.search(
    r'libkrunSource = \{\n(?P<body>.*?)\n  \};',
    text,
    re.S,
)
if block_match is None:
    raise SystemExit("failed to locate libkrunSource block in nix/pins.nix")

body = block_match.group("body")
replacements = {
    "owner": owner,
    "repo": repo,
    "rev": rev,
    "srcHash": src_hash,
    "cargoDepsHash": cargo_deps_hash,
}
for key, value in replacements.items():
    body, count = re.subn(
        rf'{key} = "[^"]+";',
        f'{key} = "{value}";',
        body,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"failed to update {key} in libkrunSource block")

updated = text[: block_match.start("body")] + body + text[block_match.end("body") :]
pins_path.write_text(updated)
PY_EOF

  cat <<REPORT_EOF
updated nix/pins.nix:
  libkrunSource.owner = "$owner";
  libkrunSource.repo = "$repo";
  libkrunSource.rev = "$rev";
  libkrunSource.srcHash = "$src_hash";
  libkrunSource.cargoDepsHash = "$cargo_deps_hash";
REPORT_EOF
}

update_prebuilt_release_pin() {
  require_commands curl jq python3

  releases_json="$(curl -fsSL "https://api.github.com/repos/$owner/$repo/releases?per_page=100")"

  jq_filter='[
    .[]
    | select(.tag_name | startswith($release_prefix))
    | select([.assets[]?.name] as $names | all($required_assets[]; . as $asset | $names | index($asset)))
  ]
  | sort_by(.published_at // .created_at)
  | last
  | .tag_name // empty'

  required_assets_json="$(
    for required_system in "${required_release_systems[@]}"; do
      printf '%s\n' "${release_assets[$required_system]}"
    done |
      jq -R . |
      jq -s .
  )"
  release_tag="$(
    printf '%s' "$releases_json" |
      jq -r --arg release_prefix "$release_prefix" --argjson required_assets "$required_assets_json" "$jq_filter"
  )"

  if [ -z "$release_tag" ]; then
    echo "failed to determine latest libkrun release tag containing all required assets:" >&2
    for required_system in "${required_release_systems[@]}"; do
      echo "  ${release_assets[$required_system]}" >&2
    done
    exit 1
  fi

  release_json="$(curl -fsSL "https://api.github.com/repos/$owner/$repo/releases/tags/$release_tag")"

  tmp_report="$(mktemp)"
  trap 'rm -f "$tmp_report"' RETURN

  for required_system in "${required_release_systems[@]}"; do
    asset_name="${release_assets[$required_system]}"
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
      echo "release $release_tag did not contain required asset $asset_name" >&2
      exit 1
    fi

    asset_hash="$(hash_url "$download_url")"
    printf '%s\t%s\t%s\n' "$required_system" "$asset_name" "$asset_hash" >> "$tmp_report"
  done

  python3 - "$pins_file" "$release_tag" "$tmp_report" <<'PY_EOF'
import re
import sys
from pathlib import Path

pins_path = Path(sys.argv[1])
release_tag = sys.argv[2]
report_path = Path(sys.argv[3])
text = pins_path.read_text()

block_match = re.search(
    r'libkrunRelease = \{\n(?P<body>.*?)\n  \};',
    text,
    re.S,
)
if block_match is None:
    raise SystemExit("failed to locate libkrunRelease block in nix/pins.nix")

body = block_match.group("body")
body, enabled_count = re.subn(r'enabled = (true|false);', 'enabled = true;', body, count=1)
if enabled_count != 1:
    raise SystemExit("failed to enable libkrunRelease in nix/pins.nix")
body, tag_count = re.subn(r'tag = "[^"]*";', f'tag = "{release_tag}";', body, count=1)
if tag_count != 1:
    raise SystemExit("failed to update libkrun release tag in nix/pins.nix")

for line in report_path.read_text().splitlines():
    system, asset_name, asset_hash = line.split("\t")
    system_pattern = re.compile(
        rf'({re.escape(system)} = \{{\n\s+asset = ")[^"]*(";\n\s+hash = ")[^"]*(";)',
        re.S,
    )
    body, system_count = system_pattern.subn(rf'\1{asset_name}\2{asset_hash}\3', body, count=1)
    if system_count != 1:
        raise SystemExit(f"failed to update libkrun release asset metadata for {system}")

updated = text[: block_match.start("body")] + body + text[block_match.end("body") :]
pins_path.write_text(updated)
PY_EOF

  echo "updated nix/pins.nix:"
  echo "  libkrunRelease.enabled = true;"
  echo "  libkrunRelease.tag = \"$release_tag\";"
  while IFS=$'\t' read -r required_system asset_name asset_hash; do
    echo "  $required_system.asset = \"$asset_name\";"
    echo "  $required_system.hash = \"$asset_hash\";"
  done < "$tmp_report"
}

case "$mode" in
  source)
    update_source_pin
    ;;
  prebuilt-release)
    update_prebuilt_release_pin
    ;;
  *)
    echo "unsupported mode: $mode" >&2
    exit 1
    ;;
esac
