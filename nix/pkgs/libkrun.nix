{
  pkgs,
  pins,
  ...
}:

let
  lib = pkgs.lib;
  system = pkgs.stdenv.hostPlatform.system;
  release = pins.libkrunRelease;
  systemPins = release.systems.${system} or (throw "unsupported libkrun system: ${system}");
in
pkgs.stdenvNoCC.mkDerivation {
  pname = "libkrun";
  version = "${release.tag}";

  nativeBuildInputs = [ pkgs.patchelf ];

  src = pkgs.fetchurl {
    url = "https://github.com/${release.owner}/${release.repo}/releases/download/${release.tag}/${systemPins.asset}";
    hash = systemPins.hash;
  };

  sourceRoot = ".";

  installPhase = ''
    runHook preInstall

    mkdir -p "$out/lib" "$out/include" "$out/lib/pkgconfig"
    cp -a lib64/. "$out/lib/"
    cp -a include/. "$out/include/"

    runHook postInstall
  '';

  postFixup = ''
    # The loftd prebuilt libkrun has DT_NEEDED on libvirglrenderer.so.1 but
    # ships no DT_RUNPATH to locate it (the previous ad8a40428d15 release did).
    # Restore that edge so crun/loftd can load libkrun without an ambient
    # LD_LIBRARY_PATH; crun's own RUNPATH cannot cover it because DT_RUNPATH is
    # not transitive across DT_NEEDED children.
    for so in "$out"/lib/libkrun.so.*; do
      if [ -L "$so" ]; then
        continue
      fi
      ${pkgs.patchelf}/bin/patchelf \
        --add-rpath ${pkgs.virglrenderer}/lib \
        "$so"
    done
  '';

  meta = {
    description = "Pinned prebuilt libkrun shared library for loftd (with krun_set_gpu_options3 render-server fd plumbing)";
    homepage = "https://github.com/${release.owner}/${release.repo}";
    license = with lib.licenses; [ asl20 ];
    platforms = lib.attrNames release.systems;
  };
}