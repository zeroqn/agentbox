{
  pkgs,
  pins,
  libkrunfw,
}:

let
  lib = pkgs.lib;
  system = pkgs.stdenv.hostPlatform.system;
  release = pins.libkrunRelease;
  systemPins =
    release.systems.${system}
      or (throw "unsupported prebuilt libkrun system or missing libkrunRelease pin: ${system}");
in
pkgs.stdenv.mkDerivation {
  pname = "libkrun";
  version = release.tag;

  src = pkgs.fetchurl {
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

    cat > "$out/lib/pkgconfig/libkrun.pc" <<PC_EOF
prefix=$out
exec_prefix=''${prefix}
libdir=''${prefix}/lib
includedir=''${prefix}/include

Name: libkrun
Description: Dynamic library for creating microVM-based process sandboxes
Version: 1.18.0
Libs: -L''${libdir} -lkrun
Cflags: -I''${includedir}
PC_EOF

    runHook postInstall
  '';

  postFixup = ''
    for lib in "$out/lib"/libkrun.so.*; do
      if [ -f "$lib" ] && [ ! -L "$lib" ]; then
        patchelf --add-rpath ${lib.getLib libkrunfw}/lib "$lib"
      fi
    done
  '';

  meta = {
    description = "Pinned prebuilt full-feature libkrun shared library for loftd and agentbox";
    homepage = "https://github.com/${release.owner}/${release.repo}";
    license = with lib.licenses; [ asl20 ];
    platforms = lib.attrNames release.systems;
  };
}
