{
  pkgs,
  pins,
  libkrunfw,
  libkrunSrc ? pkgs.fetchFromGitHub {
    inherit (pins.libkrunSource) owner repo rev;
    hash = pins.libkrunSource.srcHash;
  },
  cargoDepsHash ? pins.libkrunSource.cargoDepsHash,
  usePrebuilt ? (pins.libkrunRelease.enabled or false),
  prebuiltSrc ? null,
}:

let
  lib = pkgs.lib;
  system = pkgs.stdenv.hostPlatform.system;
  release = pins.libkrunRelease or {
    enabled = false;
    systems = { };
  };
  systemPins =
    release.systems.${system}
      or (throw "unsupported prebuilt libkrun system or missing libkrunRelease pin: ${system}");

  sourceBuild = (pkgs.libkrun.override {
    inherit libkrunfw;

    withBlk = true;
    withNet = true;
    withGpu = true;
    withSound = true;
    withInput = true;
  }).overrideAttrs (_oldAttrs: {
    version = "1.18.1-loftd-profile";
    src = libkrunSrc;
    cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
      src = libkrunSrc;
      hash = cargoDepsHash;
    };
  });

  prebuilt = pkgs.stdenv.mkDerivation {
    pname = "libkrun";
    version = release.tag;

    src =
      if prebuiltSrc != null then
        prebuiltSrc
      else
        pkgs.fetchurl {
          url = "https://github.com/${release.owner}/${release.repo}/releases/download/${release.tag}/${systemPins.asset}";
          hash = systemPins.hash;
        };

    sourceRoot = ".";
    dontBuild = true;

    nativeBuildInputs = [
      pkgs.autoPatchelfHook
      pkgs.pkg-config
    ];

    buildInputs = [
      libkrunfw
      pkgs.libcap_ng
      pkgs.libepoxy
      pkgs.libdrm
      pkgs.virglrenderer
      pkgs.pipewire
    ];

    installPhase = ''
      runHook preInstall

      mkdir -p "$out/lib" "$out/include" "$out/lib/pkgconfig"

      if [ -d lib64 ]; then
        cp -a lib64/. "$out/lib/"
      elif [ -d lib ]; then
        cp -a lib/. "$out/lib/"
      else
        cp -a libkrun.so* "$out/lib/"
      fi

      if [ -d include ]; then
        cp -a include/. "$out/include/"
      fi

      for header in libkrun.h libkrun_display.h libkrun_input.h; do
        test -f "$out/include/$header"
      done

      cat > "$out/lib/pkgconfig/libkrun.pc" <<EOF
prefix=$out
exec_prefix=\''${prefix}
libdir=\''${prefix}/lib
includedir=\''${prefix}/include

Name: libkrun
Description: Dynamic library for creating microVM-based process sandboxes
Version: 1.18.0
Libs: -L\''${libdir} -lkrun
Cflags: -I\''${includedir}
EOF

      runHook postInstall
    '';

    passthru = {
      sourcePackage = sourceBuild;
    };

    meta = {
      description = "Pinned prebuilt full-feature libkrun shared library for loftd and agentbox";
      homepage = "https://github.com/${release.owner}/${release.repo}";
      license = with lib.licenses; [ asl20 ];
      platforms = lib.attrNames release.systems;
    };
  };
in
if usePrebuilt then prebuilt else sourceBuild
