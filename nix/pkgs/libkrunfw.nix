{
  lib,
  stdenvNoCC,
  fetchurl,
  pins,
  variant ? null,
}:

assert lib.elem variant [ null ];

let
  system = stdenvNoCC.hostPlatform.system;
  release = pins.libkrunfwRelease;
  systemPins = release.systems.${system} or (throw "unsupported libkrunfw system: ${system}");
in
stdenvNoCC.mkDerivation {
  pname = "libkrunfw";
  version = release.tag;

  src = fetchurl {
    url = "https://github.com/${release.owner}/${release.repo}/releases/download/${release.tag}/${systemPins.asset}";
    hash = systemPins.hash;
  };

  sourceRoot = ".";

  installPhase = ''
    runHook preInstall

    mkdir -p $out/lib
    if [ -d lib64 ]; then
      cp -a lib64/. $out/lib/
    else
      cp -a libkrunfw.so* $out/lib/
    fi

    runHook postInstall
  '';

  meta = {
    description = "Pinned prebuilt libkrunfw guest payload shared library for agentbox";
    homepage = "https://github.com/${release.owner}/${release.repo}";
    license = with lib.licenses; [ lgpl2Only lgpl21Only ];
    platforms = lib.attrNames release.systems;
  };
}
