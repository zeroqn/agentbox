{ pkgs, fishConfig, starshipConfig, podman ? pkgs.podman, crun ? pkgs.crun, conmon ? pkgs.conmon, netavark ? pkgs.netavark, aardvarkDns ? pkgs.aardvark-dns, passt ? pkgs.passt, shadow ? pkgs.shadow }:

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


  id_in_subid_range() {
    candidate="$1"
    subid_start="$2"
    subid_count="$3"
    subid_end=$((subid_start + subid_count - 1))

    [ "$candidate" -ge "$subid_start" ] && [ "$candidate" -le "$subid_end" ]
  }

  reject_subid_overlap() {
    candidate="$1"
    candidate_name="$2"
    subid_start="$3"
    subid_count="$4"

    if id_in_subid_range "$candidate" "$subid_start" "$subid_count"; then
      echo "agentbox-entrypoint: ERROR: subordinate ID range $subid_start:$subid_count overlaps $candidate_name id $candidate" >&2
      exit 1
    fi
  }

  materialize_dev_identity_files() {
    if [ "$(id -u)" != "0" ]; then
      return 0
    fi

    if ! cat "$tmpdir/passwd" > /etc/passwd; then
      echo "agentbox-entrypoint: ERROR: failed to materialize dynamic dev entry in /etc/passwd" >&2
      exit 1
    fi
    if ! cat "$tmpdir/group" > /etc/group; then
      echo "agentbox-entrypoint: ERROR: failed to materialize dynamic dev entry in /etc/group" >&2
      exit 1
    fi
    chmod 0644 /etc/passwd /etc/group
  }

  materialize_dev_subid_files() {
    if [ "$(id -u)" != "0" ]; then
      return 0
    fi

    subid_start=100000
    subid_count=65536
    reject_subid_overlap 0 root "$subid_start" "$subid_count"
    reject_subid_overlap "$dev_uid" dev-uid "$subid_start" "$subid_count"
    reject_subid_overlap "$dev_gid" dev-gid "$subid_start" "$subid_count"

    if [ -e /etc/passwd ]; then
      while IFS= read -r protected_uid; do
        [ -n "$protected_uid" ] || continue
        reject_subid_overlap "$protected_uid" nixbld-uid "$subid_start" "$subid_count"
      done <<EOF_NIXBLD_UIDS
$(${pkgs.gawk}/bin/awk -F: '$1 ~ /^nixbld[0-9]+$/ { print $3 }' /etc/passwd)
EOF_NIXBLD_UIDS
    fi

    if [ -e /etc/group ]; then
      while IFS= read -r protected_gid; do
        [ -n "$protected_gid" ] || continue
        reject_subid_overlap "$protected_gid" nixbld-gid "$subid_start" "$subid_count"
      done <<EOF_NIXBLD_GIDS
$(${pkgs.gawk}/bin/awk -F: '$1 == "nixbld" { print $3 }' /etc/group)
EOF_NIXBLD_GIDS
    fi

    for subid_file in /etc/subuid /etc/subgid; do
      if [ -e "$subid_file" ]; then
        sed '/^dev:/d' "$subid_file" > "$tmpdir/$(basename "$subid_file")"
      else
        : > "$tmpdir/$(basename "$subid_file")"
      fi
      printf 'dev:%s:%s\n' "$subid_start" "$subid_count" >> "$tmpdir/$(basename "$subid_file")"
      if ! cat "$tmpdir/$(basename "$subid_file")" > "$subid_file"; then
        echo "agentbox-entrypoint: ERROR: failed to materialize $subid_file for rootless Podman" >&2
        exit 1
      fi
      chmod 0644 "$subid_file"
    done
  }

  install_idmap_helper() {
    src="$1"
    name="$2"
    helper_dir=/run/agentbox/idmap-bin
    dst="$helper_dir/$name"

    require_tool "$src" "$name"
    mkdir -p "$helper_dir"
    if ! ${pkgs.coreutils}/bin/install -m 4755 -o 0 -g 0 "$src" "$dst"; then
      echo "agentbox-entrypoint: ERROR: failed to install root-owned setuid $name helper at $dst" >&2
      exit 1
    fi
    if [ ! -u "$dst" ]; then
      echo "agentbox-entrypoint: ERROR: installed idmap helper '$dst' is not setuid; rootless Podman would fall back to single-UID mode" >&2
      exit 1
    fi
  }

  prepare_rootless_podman_idmap_helpers() {
    if [ "''${AGENTBOX_LIBKRUN_CONTAINERS_STORAGE:-}" != "1" ]; then
      return 0
    fi

    install_idmap_helper "${shadow}/bin/newuidmap" newuidmap
    install_idmap_helper "${shadow}/bin/newgidmap" newgidmap
    export PATH="/run/agentbox/idmap-bin:$PATH"
  }

  enable_rootless_user_namespaces() {
    if [ "''${AGENTBOX_LIBKRUN_CONTAINERS_STORAGE:-}" != "1" ]; then
      return 0
    fi
    if [ "$(id -u)" != "0" ]; then
      return 0
    fi

    userns_limit_path=/proc/sys/user/max_user_namespaces
    userns_limit_target=28633
    if [ ! -e "$userns_limit_path" ]; then
      echo "agentbox-entrypoint: ERROR: kernel does not expose $userns_limit_path; rootless Podman needs user namespace support" >&2
      exit 1
    fi

    userns_limit_current="$(cat "$userns_limit_path" 2>/dev/null || printf '0')"
    if [ -z "$userns_limit_current" ]; then
      userns_limit_current=0
    else
      case "$userns_limit_current" in
        *[!0-9]*) userns_limit_current=0 ;;
      esac
    fi
    if [ "$userns_limit_current" -lt "$userns_limit_target" ]; then
      if ! printf '%s\n' "$userns_limit_target" > "$userns_limit_path"; then
        echo "agentbox-entrypoint: ERROR: failed to set $userns_limit_path=$userns_limit_target for rootless Podman" >&2
        exit 1
      fi
    fi

    unprivileged_userns_path=/proc/sys/kernel/unprivileged_userns_clone
    if [ -e "$unprivileged_userns_path" ]; then
      unprivileged_userns_current="$(cat "$unprivileged_userns_path" 2>/dev/null || printf '0')"
      if [ "$unprivileged_userns_current" != "1" ]; then
        if ! printf '1\n' > "$unprivileged_userns_path"; then
          echo "agentbox-entrypoint: ERROR: failed to set $unprivileged_userns_path=1 for rootless Podman" >&2
          exit 1
        fi
      fi
    fi
  }

  # BEGIN agentbox passt DNS helper
  ensure_libkrun_passt_resolv_conf() {
    if [ "''${AGENTBOX_LIBKRUN_USE_PASST:-}" != "1" ]; then
      return 0
    fi

    resolv_conf="''${1:-/etc/resolv.conf}"
    passt_dns_line="nameserver 169.254.1.1"
    resolv_tmp="$tmpdir/resolv.conf.passt.$$"

    if ! printf '%s\n' "$passt_dns_line" > "$resolv_tmp"; then
      echo "agentbox-entrypoint: warning: failed to stage passt DNS resolver for '$resolv_conf'" >&2
      return 0
    fi

    if [ -e "$resolv_conf" ]; then
      if [ ! -r "$resolv_conf" ]; then
        echo "agentbox-entrypoint: warning: cannot read '$resolv_conf'; leaving passt DNS resolver unchanged" >&2
        rm -f "$resolv_tmp"
        return 0
      fi

      while IFS= read -r resolv_line || [ -n "$resolv_line" ]; do
        if [ "$resolv_line" = "$passt_dns_line" ]; then
          continue
        fi
        if ! printf '%s\n' "$resolv_line" >> "$resolv_tmp"; then
          echo "agentbox-entrypoint: warning: failed to normalize passt DNS resolver in '$resolv_conf'" >&2
          rm -f "$resolv_tmp"
          return 0
        fi
      done < "$resolv_conf"
    fi

    if ! cat "$resolv_tmp" > "$resolv_conf"; then
      echo "agentbox-entrypoint: warning: failed to write passt DNS resolver to '$resolv_conf'" >&2
      rm -f "$resolv_tmp"
      return 0
    fi

    rm -f "$resolv_tmp"
  }
  # END agentbox passt DNS helper

  find_agentbox_btrfs_disk() {
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


  bootstrap_libkrun_containers_storage() {
    if [ "$(id -u)" != "0" ]; then
      echo "agentbox-entrypoint: ERROR: libkrun container storage bootstrap must run as root" >&2
      exit 1
    fi

    require_tool "${pkgs.util-linux}/bin/blkid" "blkid"
    require_tool "${pkgs.util-linux}/bin/mount" "mount"
    require_tool "${pkgs.util-linux}/bin/findmnt" "findmnt"
    require_tool "${pkgs.btrfs-progs}/bin/btrfs" "btrfs"
    require_tool "${podman}/bin/podman" "podman"
    require_tool "${crun}/bin/crun" "crun"
    require_tool "${conmon}/bin/conmon" "conmon"
    require_tool "${netavark}/bin/netavark" "netavark"
    require_tool "${aardvarkDns}/bin/aardvark-dns" "aardvark-dns"
    require_tool "${passt}/bin/pasta" "pasta"
    require_tool "/run/agentbox/idmap-bin/newuidmap" "newuidmap"
    require_tool "/run/agentbox/idmap-bin/newgidmap" "newgidmap"

    container_disk_id="''${AGENTBOX_LIBKRUN_CONTAINERS_DISK_ID:-agentbox-containers}"
    container_disk_label="''${AGENTBOX_LIBKRUN_CONTAINERS_DISK_LABEL:-AGENTBOX_CONTAINERS}"
    containers_mount="$home_data_dir/containers"
    containers_storage_dir="$containers_mount/storage"
    containers_config_dir="$home_config_dir/containers"
    containers_run_dir="/run/user/$dev_uid"
    containers_runroot="$containers_run_dir/containers"

    mkdir -p "$containers_mount" "$containers_config_dir" "$containers_runroot"

    if ! container_disk="$(find_agentbox_btrfs_disk "$container_disk_label" "$container_disk_id")"; then
      echo "agentbox-entrypoint: ERROR: libkrun container storage btrfs disk not found (label=$container_disk_label id=$container_disk_id)" >&2
      exit 1
    fi

    if ! ${pkgs.util-linux}/bin/findmnt -rn "$containers_mount" >/dev/null 2>&1; then
      if ! ${pkgs.util-linux}/bin/mount -t btrfs "$container_disk" "$containers_mount"; then
        echo "agentbox-entrypoint: ERROR: failed to mount libkrun container storage btrfs disk '$container_disk' at '$containers_mount'" >&2
        exit 1
      fi
    fi

    if ! ${pkgs.btrfs-progs}/bin/btrfs filesystem resize max "$containers_mount" >/dev/null 2>&1; then
      echo "agentbox-entrypoint: warning: btrfs resize max failed for '$containers_mount'; continuing with existing container storage filesystem size" >&2
    fi

    mkdir -p "$containers_storage_dir" "$containers_config_dir" "$containers_runroot"
    chmod 0700 "$containers_run_dir"
    chown "$dev_uid:$dev_gid" "$containers_mount" "$containers_storage_dir" "$containers_config_dir" "$containers_run_dir" "$containers_runroot"

    if ! cat > "$containers_config_dir/storage.conf" <<EOF_STORAGE_CONF
[storage]
driver = "btrfs"
graphroot = "$containers_storage_dir"
runroot = "$containers_runroot"
EOF_STORAGE_CONF
    then
      echo "agentbox-entrypoint: ERROR: failed to write rootless Podman storage.conf at $containers_config_dir/storage.conf" >&2
      exit 1
    fi

    if ! cat > "$containers_config_dir/containers.conf" <<EOF_CONTAINERS_CONF
[engine]
cgroup_manager = "cgroupfs"
events_logger = "file"
runtime = "crun"
conmon_path = ["${conmon}/bin/conmon"]
helper_binaries_dir = ["${netavark}/bin", "${aardvarkDns}/bin", "${passt}/bin", "/run/agentbox/idmap-bin"]

[engine.runtimes]
crun = ["${crun}/bin/crun"]

[network]
network_backend = "netavark"
EOF_CONTAINERS_CONF
    then
      echo "agentbox-entrypoint: ERROR: failed to write rootless Podman containers.conf at $containers_config_dir/containers.conf" >&2
      exit 1
    fi

    if ! cat > "$containers_config_dir/registries.conf" <<EOF_REGISTRIES_CONF
[registries.block]
registries = []

[registries.insecure]
registries = []

[registries.search]
registries = ["docker.io"]
EOF_REGISTRIES_CONF
    then
      echo "agentbox-entrypoint: ERROR: failed to write rootless Podman registries.conf at $containers_config_dir/registries.conf" >&2
      exit 1
    fi

    if ! cat > "$containers_config_dir/policy.json" <<EOF_POLICY_JSON
{
  "default": [
    {
      "type": "insecureAcceptAnything"
    }
  ],
  "transports": {
    "docker-daemon": {
      "": [
        {
          "type": "insecureAcceptAnything"
        }
      ]
    }
  }
}
EOF_POLICY_JSON
    then
      echo "agentbox-entrypoint: ERROR: failed to write rootless Podman policy.json at $containers_config_dir/policy.json" >&2
      exit 1
    fi

    chown "$dev_uid:$dev_gid" "$containers_config_dir/storage.conf" "$containers_config_dir/containers.conf" "$containers_config_dir/registries.conf" "$containers_config_dir/policy.json"
    chmod 0644 "$containers_config_dir/storage.conf" "$containers_config_dir/containers.conf" "$containers_config_dir/registries.conf" "$containers_config_dir/policy.json"
    export XDG_RUNTIME_DIR="$containers_run_dir"
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

    if ! agentbox_disk="$(find_agentbox_btrfs_disk "$agentbox_disk_label" "$agentbox_disk_id")"; then
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

    mkdir -p "$agentbox_upper_dir" "$agentbox_work_dir" "$agentbox_upper_dir/store" "$agentbox_upper_dir/var"
    if [ -d "$agentbox_lower_dir/var" ]; then
      if ! ${pkgs.coreutils}/bin/cp -a --no-clobber "$agentbox_lower_dir/var/." "$agentbox_upper_dir/var/"; then
        echo "agentbox-entrypoint: ERROR: failed to preseed libkrun upperdir /nix/var from image lowerdir" >&2
        exit 1
      fi
    fi
    mkdir -p "$agentbox_upper_dir/var/nix"
    ${pkgs.coreutils}/bin/chown :nixbld "$agentbox_upper_dir/store"
    chmod 1775 "$agentbox_upper_dir/store"
    chmod 0755 "$agentbox_upper_dir/var" "$agentbox_upper_dir/var/nix"

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

  if [ "$drop_to_dev" = "1" ]; then
    materialize_dev_identity_files
    if [ "''${AGENTBOX_LIBKRUN_CONTAINERS_STORAGE:-}" = "1" ]; then
      enable_rootless_user_namespaces
      materialize_dev_subid_files
      prepare_rootless_podman_idmap_helpers
    fi
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

  if [ "''${AGENTBOX_LIBKRUN_CONTAINERS_STORAGE:-}" = "1" ]; then
    bootstrap_libkrun_containers_storage
  fi

  if [ "''${AGENTBOX_LIBKRUN_NIX_OVERLAY:-}" = "1" ]; then
    ensure_libkrun_passt_resolv_conf
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
          if [ "''${AGENTBOX_LIBKRUN_CONTAINERS_STORAGE:-}" = "1" ]; then
            if [ -z "''${XDG_RUNTIME_DIR:-}" ] || [ ! -d "$XDG_RUNTIME_DIR" ] || [ ! -w "$XDG_RUNTIME_DIR" ]; then
              echo "agentbox-entrypoint: ERROR: rootless Podman XDG_RUNTIME_DIR is not writable after dropping privileges: ''${XDG_RUNTIME_DIR:-unset}" >&2
              exit 1
            fi
            storage_conf="$HOME/.config/containers/storage.conf"
            if [ ! -f "$storage_conf" ] || ! grep -q '"'"'driver = "btrfs"'"'"' "$storage_conf"; then
              echo "agentbox-entrypoint: ERROR: rootless Podman storage.conf is missing btrfs driver at $storage_conf" >&2
              exit 1
            fi
            if grep -Eq '"'"'mount_program|driver = "(overlay|vfs)"'"'"' "$storage_conf"; then
              echo "agentbox-entrypoint: ERROR: rootless Podman storage.conf contains a forbidden overlay/vfs/fuse fallback" >&2
              exit 1
            fi
            if [ ! -w "$HOME/.local/share/containers/storage" ]; then
              echo "agentbox-entrypoint: ERROR: rootless Podman btrfs graphroot is not writable after dropping privileges" >&2
              exit 1
            fi
            if ! ${pkgs.util-linux}/bin/unshare --user --map-subids true; then
              echo "agentbox-entrypoint: ERROR: rootless Podman idmap preflight failed; check /etc/subuid, /etc/subgid, newuidmap/newgidmap, and user namespace support" >&2
              exit 1
            fi
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
