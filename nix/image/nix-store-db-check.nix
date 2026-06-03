{ pkgs, imageVariant ? "agentbox" }:

let
  toolName = if imageVariant == "loftd" then "loftd-nix-store-db-check" else "agentbox-nix-store-db-check";
  runDir = if imageVariant == "loftd" then "/run/loftd" else "/run/agentbox";
in
pkgs.writeShellScriptBin toolName ''
    set -euo pipefail

    if [ "''${1:-/nix/store}" != "/nix/store" ]; then
      echo "${toolName}: only /nix/store is supported" >&2
      exit 64
    fi

    if [ ! -d /nix/store ]; then
      echo "${toolName}: /nix/store does not exist" >&2
      exit 66
    fi

    if ! command -v nix >/dev/null 2>&1; then
      echo "${toolName}: nix is not available on PATH" >&2
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
      echo "${toolName}: failed to read Nix validity metadata with: nix path-info --all" >&2
      exit 69
    fi
    ${pkgs.coreutils}/bin/sort -u "$valid_raw" > "$valid_paths"

    ${pkgs.coreutils}/bin/comm -23 "$present_paths" "$valid_paths" > "$invalid_paths"

    if [ ! -s "$invalid_paths" ]; then
      echo "${toolName}: ok - every present /nix/store path is registered as valid"
      if [ -e /nix/store/.links ]; then
        echo "${toolName}: ignored /nix/store/.links internal link farm"
      fi
      if ${pkgs.findutils}/bin/find /nix/store -mindepth 1 -maxdepth 1 -name '*.lock' -print -quit | ${pkgs.gnugrep}/bin/grep -q .; then
        echo "${toolName}: ignored transient /nix/store/*.lock files"
      fi
      exit 0
    fi

    invalid_count="$(${pkgs.coreutils}/bin/wc -l < "$invalid_paths" | ${pkgs.coreutils}/bin/tr -d ' ')"
    echo "${toolName}: found $invalid_count present /nix/store path(s) missing from Nix validity metadata:" >&2
    ${pkgs.gnused}/bin/sed 's/^/  /' "$invalid_paths" >&2

    libkrun_upper_dir="${runDir}/nix-disk/upper"
    libkrun_upper_store_dir="$libkrun_upper_dir/store"
    libkrun_upper_var_nix_dir="$libkrun_upper_dir/var/nix"
    libkrun_upper_db_dir="$libkrun_upper_var_nix_dir/db"
    upper_present_message="store object present in libkrun upperdir"
    upper_absent_message="store object not found in libkrun upperdir; may come from lower image or another mounted view"
    upper_unavailable_message="upperdir unavailable; overlay source evidence not inspected"
    evidence_caveat="store-layer evidence only; not root-cause evidence"

    echo "${toolName}: libkrun upperdir diagnostics: $evidence_caveat" >&2
    if [ -d "$libkrun_upper_dir" ] && [ -r "$libkrun_upper_dir" ] && [ -x "$libkrun_upper_dir" ]; then
      echo "${toolName}: inspecting libkrun upperdir: $libkrun_upper_dir" >&2
      if [ -d "$libkrun_upper_store_dir" ] && [ -r "$libkrun_upper_store_dir" ] && [ -x "$libkrun_upper_store_dir" ]; then
        if ${pkgs.findutils}/bin/find "$libkrun_upper_store_dir" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null | ${pkgs.gnugrep}/bin/grep -q .; then
          while IFS= read -r store_path; do
            store_name="$(${pkgs.coreutils}/bin/basename "$store_path")"
            upper_store_candidate="$libkrun_upper_store_dir/$store_name"
            if [ -e "$upper_store_candidate" ]; then
              echo "  $store_path: $upper_present_message ($upper_store_candidate)" >&2
            else
              echo "  $store_path: $upper_absent_message" >&2
            fi
          done < "$invalid_paths"
        else
          echo "${toolName}: upper store subdir unavailable/empty: $libkrun_upper_store_dir" >&2
          while IFS= read -r store_path; do
            echo "  $store_path: $upper_absent_message" >&2
          done < "$invalid_paths"
        fi
      else
        echo "${toolName}: upper store subdir unavailable/empty: $libkrun_upper_store_dir" >&2
        echo "${toolName}: overlay source evidence not inspected for individual invalid paths" >&2
      fi

      if [ -d "$libkrun_upper_db_dir" ]; then
        echo "${toolName}: metadata-shadow context only: upper /var/nix/db exists at $libkrun_upper_db_dir and may shadow lower Nix metadata" >&2
      elif [ -d "$libkrun_upper_var_nix_dir" ]; then
        echo "${toolName}: metadata-shadow context only: upper /var/nix exists at $libkrun_upper_var_nix_dir and may shadow lower Nix metadata" >&2
      else
        echo "${toolName}: metadata-shadow context only: no upper /var/nix metadata directory was found" >&2
      fi
    else
      echo "${toolName}: $upper_unavailable_message: $libkrun_upper_dir" >&2
    fi

    if command -v nix-store >/dev/null 2>&1; then
      echo "${toolName}: nix-store --verify-path evidence:" >&2
      while IFS= read -r store_path; do
        echo "  $store_path" >&2
        if env -u LD_PRELOAD -u NSS_WRAPPER_PASSWD -u NSS_WRAPPER_GROUP nix-store --verify-path "$store_path" > "$verify_log" 2>&1; then
          ${pkgs.gnused}/bin/sed 's/^/    /' "$verify_log" >&2
        else
          ${pkgs.gnused}/bin/sed 's/^/    /' "$verify_log" >&2
        fi
      done < "$invalid_paths"
    fi

    echo "${toolName}: no repair was attempted" >&2
    exit 1
  ''
