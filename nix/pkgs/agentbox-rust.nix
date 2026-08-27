{
  self,
  pkgs,
  pins,
  libkrun ? null,
  libkrunfw ? null,
  enableCiSccache ? false,
}:
let
  agentboxVersion = pins.agentboxVersion;
  ciSccacheNativeBuildInputs = pkgs.lib.optionals enableCiSccache [ pkgs.sccache ];
  ciSccacheEnv = pkgs.lib.optionalAttrs enableCiSccache {
    RUSTC_WRAPPER = "${pkgs.sccache}/bin/sccache";
    SCCACHE_DIR = "/nix/var/cache/sccache";
    SCCACHE_IGNORE_SERVER_IO_ERROR = "1";
  };
  runtimeLibraryPath = pkgs.lib.makeLibraryPath (
    pkgs.lib.optionals (libkrun != null) [ (pkgs.lib.getLib libkrun) ]
    ++ pkgs.lib.optionals (libkrunfw != null) [ (pkgs.lib.getLib libkrunfw) ]
  );
  runtimePath = pkgs.lib.makeBinPath [
    pkgs.buildah
    pkgs.btrfs-progs
    pkgs.fuse-overlayfs
    pkgs.passt
    pkgs.strace
    pkgs.util-linux
  ];
  renderServerIcdPath =
    "${pkgs.mesa}/share/vulkan/icd.d/radeon_icd.${pkgs.stdenv.hostPlatform.parsed.cpu.name}.json";
  renderServerWrapperArgs =
    [
      "--set"
      "LOFTD_MESA_LIBDIR"
      "${pkgs.mesa}/lib"
      "--set"
      "LOFTD_MESA_ICD"
      renderServerIcdPath
      "--set"
      "LOFTD_VULKAN_LOADER_LIBDIR"
      "${pkgs.vulkan-loader}/lib"
    ];
  runtimeWrapperArgs =
    [
      "--prefix"
      "PATH"
      ":"
      runtimePath
    ]
    ++ pkgs.lib.optionals (runtimeLibraryPath != "") [
      "--prefix"
      "LD_LIBRARY_PATH"
      ":"
      runtimeLibraryPath
    ];

  rustPackage = pkgs.rustPlatform.buildRustPackage ({
    pname = "agentbox";
    version = agentboxVersion;
    src = self;

    nativeBuildInputs = [ pkgs.makeWrapper ] ++ ciSccacheNativeBuildInputs;

    cargoLock = {
      lockFile = ../../Cargo.lock;
    };

    postInstall = ''
      mkdir -p "$out/libexec/loftd-helpers" "$out/lib/loftd"
      install -Dm644 ${self}/crates/loftd/assets/seccomp/default.json "$out/share/loftd/seccomp/default.json"
      install -Dm644 ${self}/crates/loftd/assets/seccomp/render-server.json "$out/share/loftd/seccomp/render-server.json"
      ln -s ${pkgs.buildah}/bin/buildah "$out/libexec/loftd-helpers/buildah"
      ln -s ${pkgs.btrfs-progs}/bin/btrfs "$out/libexec/loftd-helpers/btrfs"
      ln -s ${pkgs.btrfs-progs}/bin/mkfs.btrfs "$out/libexec/loftd-helpers/mkfs.btrfs"
      ln -s ${pkgs.util-linux}/bin/blkid "$out/libexec/loftd-helpers/blkid"
      ln -s ${pkgs.passt}/bin/pasta "$out/libexec/loftd-helpers/pasta"
      ln -s ${pkgs.passt}/bin/passt "$out/libexec/loftd-helpers/passt"
      ln -s ${pkgs.strace}/bin/strace "$out/libexec/loftd-helpers/strace"
      ln -s ${pkgs.virglrenderer}/libexec/virgl_render_server "$out/libexec/loftd-helpers/virgl_render_server"
      ${pkgs.lib.optionalString (libkrun != null) ''
        for library in ${pkgs.lib.getLib libkrun}/lib/libkrun.so*; do
          ln -s "$library" "$out/lib/loftd/$(basename "$library")"
        done
      ''}
      ${pkgs.lib.optionalString (libkrunfw != null) ''
        for library in ${pkgs.lib.getLib libkrunfw}/lib/libkrunfw.so*; do
          ln -s "$library" "$out/lib/loftd/$(basename "$library")"
        done
      ''}
      wrapProgram "$out/bin/agentbox" ${pkgs.lib.escapeShellArgs (runtimeWrapperArgs ++ renderServerWrapperArgs)}
    '';
  } // ciSccacheEnv);

  muslTarget =
    if pkgs.stdenv.hostPlatform.system == "x86_64-linux" then
      "x86_64-unknown-linux-musl"
    else if pkgs.stdenv.hostPlatform.system == "aarch64-linux" then
      "aarch64-unknown-linux-musl"
    else
      throw "agentbox-musl is only supported on Linux";

  agentboxMuslPackage = pkgs.pkgsStatic.rustPlatform.buildRustPackage ({
    pname = "agentbox";
    version = agentboxVersion;
    src = self;

    nativeBuildInputs = ciSccacheNativeBuildInputs;

    cargoLock = {
      lockFile = ../../Cargo.lock;
    };

    CARGO_BUILD_TARGET = muslTarget;
    cargoBuildFlags = [
      "--package"
      "agentbox-host"
      "--package"
      "agentbox-guest-init"
      "--package"
      "loftd-guest-init"
    ];
    cargoTestFlags = [
      "--package"
      "agentbox-host"
      "--package"
      "agentbox-guest-init"
      "--package"
      "loftd-guest-init"
    ];
  } // ciSccacheEnv);
in
{
  inherit rustPackage agentboxMuslPackage;
}
