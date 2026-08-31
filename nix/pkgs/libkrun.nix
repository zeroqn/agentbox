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

  meta = {
    description = "Pinned prebuilt libkrun shared library for loftd (with krun_set_gpu_options3 render-server fd plumbing)";
    homepage = "https://github.com/${release.owner}/${release.repo}";
    license = with lib.licenses; [ asl20 ];
    platforms = lib.attrNames release.systems;
  };
}