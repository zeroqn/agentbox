{ pkgs, fishConfig, starshipConfig }:

pkgs.writeShellScriptBin "agentbox-entrypoint" ''
  set -euo pipefail

  export USER=dev
  export HOME=/home/dev
  export SHELL=${pkgs.fish}/bin/fish
  export XDG_CACHE_HOME="$HOME/.cache"
  runtime_uid="$(id -u)"
  runtime_gid="$(id -g)"

  tmpdir="$(mktemp -d)"
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
      mkdir -p "$shadow"
      cp -RL "$path/." "$shadow/" 2>/dev/null || true
      rm -rf "$path"
      mkdir -p "$path"
      cp -RL "$shadow/." "$path/" 2>/dev/null || true
    fi
  }

  cat /etc/passwd > "$tmpdir/passwd"
  cat /etc/group > "$tmpdir/group"
  chmod u+w "$tmpdir/passwd" "$tmpdir/group"
  printf 'dev:x:%s:%s:dev user:%s:%s\n' "$runtime_uid" "$runtime_gid" "$HOME" "$SHELL" >> "$tmpdir/passwd"
  printf 'dev:x:%s:\n' "$runtime_gid" >> "$tmpdir/group"

  export NSS_WRAPPER_PASSWD="$tmpdir/passwd"
  export NSS_WRAPPER_GROUP="$tmpdir/group"
  if [ -n "''${LD_PRELOAD-}" ]; then
    export LD_PRELOAD="${pkgs.nss_wrapper}/lib/libnss_wrapper.so:$LD_PRELOAD"
  else
    export LD_PRELOAD="${pkgs.nss_wrapper}/lib/libnss_wrapper.so"
  fi

  home_config_dir="$HOME/.config"
  home_cache_dir="$XDG_CACHE_HOME"
  fish_config_dir="$home_config_dir/fish"
  bundled_fish_conf="${fishConfig}/share/agentbox/fish/conf.d/agentbox-starship.fish"
  bundled_starship_config="${starshipConfig}/share/agentbox/starship.toml"

  materialize_writable_dir "$home_cache_dir" "$tmpdir/home-cache"
  chmod u+w "$home_cache_dir" 2>/dev/null || true
  materialize_writable_dir "$home_config_dir" "$tmpdir/home-config"
  if [ ! -e "$home_config_dir/starship.toml" ]; then
    cp "$bundled_starship_config" "$home_config_dir/starship.toml"
  fi
  materialize_writable_dir "$fish_config_dir" "$tmpdir/fish-config"
  mkdir -p "$fish_config_dir/conf.d"
  chmod u+w "$fish_config_dir" "$fish_config_dir/conf.d" 2>/dev/null || true
  if [ ! -e "$fish_config_dir/conf.d/agentbox-starship.fish" ]; then
    cp "$bundled_fish_conf" "$fish_config_dir/conf.d/agentbox-starship.fish"
  fi

  if [ "$#" -eq 0 ]; then
    set -- ${pkgs.fish}/bin/fish -l
  fi

  exec "$@"
''
