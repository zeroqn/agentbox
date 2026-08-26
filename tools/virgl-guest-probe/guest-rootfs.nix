{ pkgs, guestProbe }:
let
  # The runtime the guest needs at boot.  glibc/mesa/vulkan-loader are added as
  # explicit closure roots so their libs (libc, ld-linux, libvulkan_virtio.so,
  # libvulkan.so.1, the virtio ICD json, DRM/dri bits) are all present under the
  # rootfs's /nix/store.  busybox supplies the static sh/mount/mkdir applets that
  # /init uses before the probe runs.
  runtime = pkgs.buildEnv {
    name = "virgl-guest-runtime";
    paths = [ pkgs.busybox guestProbe ];
    ignoreCollisions = true;
  };

  # Full closure of everything the guest touches, so every absolute
  # /nix/store/<hash> path referenced by libs, the ICD json, and the probe's
  # dynamic loader exists inside the rootfs at the identical store path.
  closure = pkgs.closureInfo {
    rootPaths = [ runtime pkgs.glibc pkgs.mesa pkgs.vulkan-loader ];
  };
in
pkgs.stdenv.mkDerivation {
  name = "virgl-guest-rootfs";
  dontConfigure = true;
  dontBuild = true;

  buildCommand = ''
    echo "assembling guest rootfs"
    for d in dev proc sys tmp run etc bin; do
      mkdir -p "$out/$d"
    done

    # Copy the full store closure into $out/nix/store.  Names are preserved, so
    # absolute /nix/store/<hash> paths resolve identically inside the guest.
    mkdir -p "$out/nix/store"
    while read -r p; do
      [[ -z "$p" ]] && continue
      cp -a "$p" "$out/nix/store/"
    done < "${closure}/store-paths"

    BB=${pkgs.busybox}/bin/busybox
    MESA_ICD=${pkgs.mesa}/share/vulkan/icd.d/virtio_icd.x86_64.json

    # Busybox is a single multi-call binary; the applets (/bin/sh etc.) are
    # dispatched by argv[0].  The kernel resolves the /init shebang (#!/bin/sh)
    # through /bin/sh, so a real symlink must exist inside the rootfs.
    for applet in sh mount mkdir ls cat grep sed; do
      ln -s "$BB" "$out/bin/$applet"
    done

    cat > "$out/init" <<EOF
    #!/bin/sh
    # PID 1 in the guest: set up a usable /dev, /proc, /sys, then run the probe
    # with the Mesa venus (virtio) discovery env so the venus ICD is what
    # libvulkan.so.1 loads.
    BB=$BB
    MESA_ICD=$MESA_ICD
    MESA_LIB=${pkgs.mesa}/lib
    VK_LOAD_LIB=${pkgs.vulkan-loader}/lib
    GLIBC_LIB=${pkgs.glibc}/lib

    "\$BB" mount -t proc proc /proc 2>/dev/null
    "\$BB" mount -t sysfs sysfs /sys 2>/dev/null
    "\$BB" mount -t devtmpfs devtmpfs /dev 2>/dev/null
    "\$BB" mkdir -p /dev/dri /tmp

    export VK_DRIVER_FILES="\$MESA_ICD"
    export LD_LIBRARY_PATH="\$VK_LOAD_LIB:\$MESA_LIB:\$GLIBC_LIB"
    export XDG_RUNTIME_DIR=/tmp

    echo "[init] guest boot; VK_DRIVER_FILES=\$VK_DRIVER_FILES"
    echo "[init] /dev/dri:"; "\$BB" ls /dev/dri 2>/dev/null

    exec ${guestProbe}/bin/guest-probe
    EOF
    chmod +x "$out/init"

    echo "rootfs assembled at $out"
    "$BB" du -sh "$out" 2>/dev/null || true
  '';

  nativeBuildInputs = [ pkgs.busybox ];
}