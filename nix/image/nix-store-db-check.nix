{ pkgs }:

pkgs.writeShellScriptBin "agentbox-nix-store-db-check" ''
    set -euo pipefail

    if [ "''${1:-/nix/store}" != "/nix/store" ]; then
      echo "agentbox-nix-store-db-check: only /nix/store is supported" >&2
      exit 64
    fi

    if [ ! -d /nix/store ]; then
      echo "agentbox-nix-store-db-check: /nix/store does not exist" >&2
      exit 66
    fi

    if ! command -v nix >/dev/null 2>&1; then
      echo "agentbox-nix-store-db-check: nix is not available on PATH" >&2
      exit 69
    fi

    present_raw="$(${pkgs.coreutils}/bin/mktemp)"
    present_paths="$(${pkgs.coreutils}/bin/mktemp)"
    valid_raw="$(${pkgs.coreutils}/bin/mktemp)"
    valid_paths="$(${pkgs.coreutils}/bin/mktemp)"
    invalid_paths="$(${pkgs.coreutils}/bin/mktemp)"
    verify_log="$(${pkgs.coreutils}/bin/mktemp)"
    trap '${pkgs.coreutils}/bin/rm -f "$present_raw" "$present_paths" "$valid_raw" "$valid_paths" "$invalid_paths" "$verify_log"' EXIT

    ${pkgs.findutils}/bin/find /nix/store -mindepth 1 -maxdepth 1 ! -name .links ! -name '*.lock' -print > "$present_raw"
    ${pkgs.gnugrep}/bin/grep -E '^/nix/store/[0-9a-df-np-sv-z]{32}-.+' "$present_raw" \
      | ${pkgs.coreutils}/bin/sort -u > "$present_paths" || true

    if ! nix path-info --all > "$valid_raw"; then
      echo "agentbox-nix-store-db-check: failed to read Nix validity metadata with: nix path-info --all" >&2
      exit 69
    fi
    ${pkgs.coreutils}/bin/sort -u "$valid_raw" > "$valid_paths"

    ${pkgs.coreutils}/bin/comm -23 "$present_paths" "$valid_paths" > "$invalid_paths"

    if [ ! -s "$invalid_paths" ]; then
      echo "agentbox-nix-store-db-check: ok - every present /nix/store path is registered as valid"
      if [ -e /nix/store/.links ]; then
        echo "agentbox-nix-store-db-check: ignored /nix/store/.links internal link farm"
      fi
      if ${pkgs.findutils}/bin/find /nix/store -mindepth 1 -maxdepth 1 -name '*.lock' -print -quit | ${pkgs.gnugrep}/bin/grep -q .; then
        echo "agentbox-nix-store-db-check: ignored transient /nix/store/*.lock files"
      fi
      exit 0
    fi

    invalid_count="$(${pkgs.coreutils}/bin/wc -l < "$invalid_paths" | ${pkgs.coreutils}/bin/tr -d ' ')"
    echo "agentbox-nix-store-db-check: found $invalid_count present /nix/store path(s) missing from Nix validity metadata:" >&2
    ${pkgs.gnused}/bin/sed 's/^/  /' "$invalid_paths" >&2

    if command -v nix-store >/dev/null 2>&1; then
      echo "agentbox-nix-store-db-check: nix-store --verify-path evidence:" >&2
      while IFS= read -r store_path; do
        echo "  $store_path" >&2
        if env -u LD_PRELOAD -u NSS_WRAPPER_PASSWD -u NSS_WRAPPER_GROUP nix-store --verify-path "$store_path" > "$verify_log" 2>&1; then
          ${pkgs.gnused}/bin/sed 's/^/    /' "$verify_log" >&2
        else
          ${pkgs.gnused}/bin/sed 's/^/    /' "$verify_log" >&2
        fi
      done < "$invalid_paths"
    fi

    echo "agentbox-nix-store-db-check: no repair was attempted" >&2
    exit 1
  ''
