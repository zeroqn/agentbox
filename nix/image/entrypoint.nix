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

  require_tool() {
    tool_path="$1"
    tool_name="$2"
    if [ ! -x "$tool_path" ]; then
      echo "agentbox-entrypoint: ERROR: required tool '$tool_name' is not available at '$tool_path'" >&2
      exit 1
    fi
  }

  find_agentbox_nix_disk() {
    disk_label="$1"
    disk_id="$2"

    if disk_path="$(${pkgs.util-linux}/bin/blkid -L "$disk_label" 2>/dev/null)" && [ -n "$disk_path" ]; then
      printf '%s\n' "$disk_path"
      return 0
    fi

    for candidate in /dev/disk/by-id/*"$disk_id"* /dev/vd? /dev/sd? /dev/xvd? /dev/nvme?n? /dev/pmem?; do
      [ -e "$candidate" ] || continue
      if candidate_label="$(${pkgs.util-linux}/bin/blkid -o value -s LABEL "$candidate" 2>/dev/null)" \
        && [ "$candidate_label" = "$disk_label" ]; then
        printf '%s\n' "$candidate"
        return 0
      fi
    done

    return 1
  }

  bootstrap_libkrun_nix_overlay() {
    if [ "$(id -u)" != "0" ]; then
      echo "agentbox-entrypoint: ERROR: libkrun /nix overlay bootstrap must run as root" >&2
      exit 1
    fi

    require_tool "${pkgs.util-linux}/bin/blkid" "blkid"
    require_tool "${pkgs.util-linux}/bin/mount" "mount"
    require_tool "${pkgs.btrfs-progs}/bin/btrfs" "btrfs"
    require_tool "${pkgs.nix}/bin/nix-daemon" "nix-daemon"

    agentbox_disk_id="''${AGENTBOX_LIBKRUN_NIX_DISK_ID:-agentbox-nix}"
    agentbox_disk_label="''${AGENTBOX_LIBKRUN_NIX_DISK_LABEL:-AGENTBOX_NIX}"
    agentbox_run_dir="/run/agentbox"
    agentbox_disk_mount="$agentbox_run_dir/nix-disk"
    agentbox_lower_dir="$agentbox_run_dir/nix-lower"
    agentbox_upper_dir="$agentbox_disk_mount/upper"
    agentbox_work_dir="$agentbox_disk_mount/work"
    agentbox_socket="/nix/var/nix/daemon-socket/socket"

    mkdir -p "$agentbox_run_dir" "$agentbox_disk_mount" "$agentbox_lower_dir"

    if ! agentbox_disk="$(find_agentbox_nix_disk "$agentbox_disk_label" "$agentbox_disk_id")"; then
      echo "agentbox-entrypoint: ERROR: libkrun /nix btrfs disk not found (label=$agentbox_disk_label id=$agentbox_disk_id)" >&2
      exit 1
    fi

    if ! ${pkgs.util-linux}/bin/findmnt -rn "$agentbox_lower_dir" >/dev/null 2>&1; then
      if ! ${pkgs.util-linux}/bin/mount --bind /nix "$agentbox_lower_dir"; then
        echo "agentbox-entrypoint: ERROR: failed to preserve image /nix lowerdir at $agentbox_lower_dir" >&2
        exit 1
      fi
      if ! ${pkgs.util-linux}/bin/mount -o remount,bind,ro "$agentbox_lower_dir"; then
        echo "agentbox-entrypoint: ERROR: failed to make image /nix lowerdir read-only at $agentbox_lower_dir" >&2
        exit 1
      fi
    fi

    if ! ${pkgs.util-linux}/bin/findmnt -rn "$agentbox_disk_mount" >/dev/null 2>&1; then
      if ! ${pkgs.util-linux}/bin/mount -t btrfs "$agentbox_disk" "$agentbox_disk_mount"; then
        echo "agentbox-entrypoint: ERROR: failed to mount libkrun /nix btrfs disk '$agentbox_disk' at '$agentbox_disk_mount'" >&2
        exit 1
      fi
    fi

    if ! ${pkgs.btrfs-progs}/bin/btrfs filesystem resize max "$agentbox_disk_mount" >/dev/null 2>&1; then
      echo "agentbox-entrypoint: warning: btrfs resize max failed for '$agentbox_disk_mount'; continuing with existing filesystem size" >&2
    fi

    mkdir -p "$agentbox_upper_dir" "$agentbox_work_dir"

    if ! ${pkgs.util-linux}/bin/mount -t overlay overlay \
      -o "lowerdir=$agentbox_lower_dir,upperdir=$agentbox_upper_dir,workdir=$agentbox_work_dir" \
      /nix; then
      echo "agentbox-entrypoint: ERROR: failed to mount libkrun overlay at /nix" >&2
      exit 1
    fi

    mkdir -p /nix/var/nix/daemon-socket
    ${pkgs.nix}/bin/nix-daemon &
    agentbox_nix_daemon_pid="$!"

    for _ in $(seq 1 100); do
      if [ -S "$agentbox_socket" ]; then
        export NIX_REMOTE="unix://$agentbox_socket"
        return 0
      fi
      if ! kill -0 "$agentbox_nix_daemon_pid" 2>/dev/null; then
        echo "agentbox-entrypoint: ERROR: nix-daemon exited before creating '$agentbox_socket'" >&2
        exit 1
      fi
      sleep 0.1
    done

    echo "agentbox-entrypoint: ERROR: nix-daemon did not create '$agentbox_socket' before timeout" >&2
    exit 1
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

  if [ "''${AGENTBOX_LIBKRUN_NIX_OVERLAY:-}" = "1" ]; then
    bootstrap_libkrun_nix_overlay
  fi

  if [ "$drop_to_dev" = "1" ]; then
    if [ "''${AGENTBOX_LIBKRUN_NIX_OVERLAY:-}" = "1" ]; then
      exec ${pkgs.util-linux}/bin/setpriv --reuid="$dev_uid" --regid="$dev_gid" --clear-groups \
        ${pkgs.bashInteractive}/bin/bash -c '
          socket_path="''${NIX_REMOTE#unix://}"
          if [ -z "$socket_path" ] || [ ! -S "$socket_path" ]; then
            echo "agentbox-entrypoint: ERROR: libkrun in-guest nix-daemon socket is not accessible after dropping privileges: $socket_path" >&2
            exit 1
          fi
          exec "$@"
        ' bash "$@"
    fi

    agentbox_proxy_sock="/tmp/agentbox-nix-daemon.sock"
    agentbox_proxy_host="''${AGENTBOX_NIX_PROXY_HOST:-}"
    agentbox_proxy_port="''${AGENTBOX_NIX_PROXY_PORT:-19876}"

    if [ -z "$agentbox_proxy_host" ]; then
      echo "agentbox-entrypoint: ERROR: AGENTBOX_NIX_PROXY_HOST is not set; the agentbox binary may be outdated. Rebuild and try again." >&2
      exit 1
    fi

    exec ${pkgs.util-linux}/bin/setpriv --reuid="$dev_uid" --regid="$dev_gid" --clear-groups \
      ${pkgs.bashInteractive}/bin/bash -c '
        agentbox_proxy_sock="$1"; shift
        agentbox_proxy_host="$1"; shift
        agentbox_proxy_port="$1"; shift

        ${pkgs.socat}/bin/socat UNIX-LISTEN:"$agentbox_proxy_sock",fork,unlink-early,umask=000 \
          TCP:"$agentbox_proxy_host:$agentbox_proxy_port" &

        for _ in $(seq 1 50); do
          if [ -S "$agentbox_proxy_sock" ]; then break; fi
          sleep 0.1
        done

        exec "$@"
      ' bash "$agentbox_proxy_sock" "$agentbox_proxy_host" "$agentbox_proxy_port" "$@"
  fi

  exec "$@"
''
