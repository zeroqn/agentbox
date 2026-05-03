{ pkgs, fishConfig, starshipConfig }:

pkgs.writeShellScriptBin "agentbox-entrypoint" ''
  set -euo pipefail

  export USER=dev
  export HOME=/home/dev
  export SHELL=${pkgs.fish}/bin/fish
  export XDG_CONFIG_HOME="$HOME/.config"
  export XDG_DATA_HOME="$HOME/.local/share"
  export XDG_CACHE_HOME="$HOME/.cache"
  user_tmpdir="$XDG_CACHE_HOME/tmp"
  if [ "$#" -eq 0 ]; then
    set -- ${pkgs.fish}/bin/fish -l
  fi

  command_basename="''${1##*/}"
  runtime_uid="$(id -u)"
  runtime_gid="$(id -g)"
  dev_uid="$runtime_uid"
  dev_gid="$runtime_gid"
  if [ "$runtime_uid" = "0" ]; then
    dev_uid=1000
    dev_gid=1000
    if [ -n "''${AGENTBOX_HOST_UID:-}" ] && [ -n "''${AGENTBOX_HOST_GID:-}" ]; then
      dev_uid="$AGENTBOX_HOST_UID"
      dev_gid="$AGENTBOX_HOST_GID"
    fi
  fi
  interactive_fish_task=0
  if [ "$command_basename" = "fish" ] && [ "''${2:-}" = "-l" ]; then
    interactive_fish_task=1
  fi
  drop_to_dev=0
  if [ "$runtime_uid" = "0" ] \
    && { [ "''${AGENTBOX_KVM_DROP_TO_DEV:-}" = "1" ] || [ "$interactive_fish_task" = "1" ]; }; then
    drop_to_dev=1
  fi
  if [ "$drop_to_dev" = "1" ]; then
    if [ -z "''${AGENTBOX_HOST_UID:-}" ] || [ -z "''${AGENTBOX_HOST_GID:-}" ]; then
      echo "agentbox-entrypoint: ERROR: AGENTBOX_HOST_UID and AGENTBOX_HOST_GID are required for KVM task mode" >&2
      echo "agentbox-entrypoint: The agentbox binary may be outdated. Rebuild and try again." >&2
      exit 1
    fi
  fi

  tmpdir="$(TMPDIR=/tmp mktemp -d)"
  cleanup() {
    rm -rf "$tmpdir"
  }
  trap cleanup EXIT

  materialize_writable_dir() {
    path="$1"
    shadow="$2"

    if [ ! -e "$path" ]; then
      mkdir -p "$path"
      return 0
    fi

    if [ -L "$path" ] || [ ! -w "$path" ]; then
      if ! mkdir -p "$shadow" || ! cp -RL "$path/." "$shadow/" 2>/dev/null; then
        echo "agentbox-entrypoint: warning: cannot shadow '$path' to writable layer" >&2
        return 0
      fi
      rm -rf "$path"
      mkdir -p "$path"
      if ! cp -RL "$shadow/." "$path/" 2>/dev/null; then
        echo "agentbox-entrypoint: warning: failed to materialize writable dir '$path'" >&2
      fi
    fi
  }

  if [ -e /etc/passwd ]; then
    sed '/^dev:/d' /etc/passwd > "$tmpdir/passwd"
  else
    : > "$tmpdir/passwd"
  fi
  if [ -e /etc/group ]; then
    sed '/^dev:/d' /etc/group > "$tmpdir/group"
  else
    : > "$tmpdir/group"
  fi
  chmod u+w "$tmpdir/passwd" "$tmpdir/group"
  printf 'dev:x:%s:%s:dev user:%s:%s\n' "$dev_uid" "$dev_gid" "$HOME" "$SHELL" >> "$tmpdir/passwd"
  printf 'dev:x:%s:\n' "$dev_gid" >> "$tmpdir/group"
  if [ "$drop_to_dev" = "1" ]; then
    chmod 0755 "$tmpdir"
    chmod 0644 "$tmpdir/passwd" "$tmpdir/group"
  fi

  export NSS_WRAPPER_PASSWD="$tmpdir/passwd"
  export NSS_WRAPPER_GROUP="$tmpdir/group"
  if [ -n "''${LD_PRELOAD-}" ]; then
    export LD_PRELOAD="${pkgs.nss_wrapper}/lib/libnss_wrapper.so:$LD_PRELOAD"
  else
    export LD_PRELOAD="${pkgs.nss_wrapper}/lib/libnss_wrapper.so"
  fi

  home_config_dir="$XDG_CONFIG_HOME"
  home_data_dir="$XDG_DATA_HOME"
  home_cache_dir="$XDG_CACHE_HOME"
  fish_config_dir="$home_config_dir/fish"
  fish_data_dir="$home_data_dir/fish"
  starship_cache_dir="$home_cache_dir/starship"
  bundled_fish_conf="${fishConfig}/share/agentbox/fish/conf.d/agentbox-starship.fish"
  bundled_starship_config="${starshipConfig}/share/agentbox/starship.toml"

  materialize_writable_dir "$home_config_dir" "$tmpdir/home-config"
  materialize_writable_dir "$home_data_dir" "$tmpdir/home-data"
  if [ ! -e "$home_config_dir/starship.toml" ]; then
    cp "$bundled_starship_config" "$home_config_dir/starship.toml"
  fi
  materialize_writable_dir "$fish_config_dir" "$tmpdir/fish-config"
  mkdir -p \
    "$fish_config_dir/conf.d" \
    "$fish_config_dir/completions" \
    "$fish_config_dir/functions" \
    "$fish_data_dir" \
    "$starship_cache_dir" \
    "$user_tmpdir"
  chmod u+w \
    "$fish_config_dir" \
    "$fish_config_dir/conf.d" \
    "$fish_config_dir/completions" \
    "$fish_config_dir/functions" \
    "$fish_data_dir" \
    "$starship_cache_dir" \
    "$user_tmpdir" \
    2>/dev/null || true
  if [ ! -e "$fish_config_dir/conf.d/agentbox-starship.fish" ]; then
    cp "$bundled_fish_conf" "$fish_config_dir/conf.d/agentbox-starship.fish"
  fi
  if [ "$drop_to_dev" = "1" ]; then
    chown -R "$dev_uid:$dev_gid" "$home_config_dir" "$home_data_dir" "$starship_cache_dir" "$user_tmpdir" 2>/dev/null \
      || echo "agentbox-entrypoint: warning: chown home config dirs failed (may be read-only)" >&2
    chown "$dev_uid:$dev_gid" "$home_cache_dir" 2>/dev/null \
      || echo "agentbox-entrypoint: warning: chown home cache dir failed (may be read-only)" >&2
  fi

  export TMPDIR="$user_tmpdir"

  if [ "$drop_to_dev" = "1" ]; then
    exec ${pkgs.util-linux}/bin/setpriv --reuid="$dev_uid" --regid="$dev_gid" --clear-groups "$@"
  fi

  exec "$@"
''
