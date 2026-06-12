{ pkgs, pins }:
let
  ompPrebuiltRelease = pins.ompPrebuiltRelease;
  prebuiltSystem = pkgs.stdenv.hostPlatform.system;
  runtimeLibs = [
    pkgs.stdenv.cc.cc.lib
    pkgs.stdenv.cc.libc
  ];
in
if builtins.hasAttr prebuiltSystem ompPrebuiltRelease.systems then
  let
    assetInfo = builtins.getAttr prebuiltSystem ompPrebuiltRelease.systems;
    releaseUrl =
      "https://github.com/${ompPrebuiltRelease.owner}/${ompPrebuiltRelease.repo}/releases/download/${ompPrebuiltRelease.tag}/${assetInfo.asset}";
  in
  pkgs.stdenvNoCC.mkDerivation {
    pname = "omp";
    version = pkgs.lib.removePrefix "v" ompPrebuiltRelease.tag;
    src = pkgs.fetchurl {
      url = releaseUrl;
      hash = assetInfo.hash;
    };
    dontUnpack = true;

    nativeBuildInputs = [
      pkgs.autoPatchelfHook
      pkgs.binutils
    ];

    buildInputs = runtimeLibs;

    installPhase = ''
      runHook preInstall

      readelf -h "$src" >/dev/null
      install -Dm755 "$src" "$out/bin/omp"

      runHook postInstall
    '';

    passthru = {
      inherit releaseUrl;
      releaseTag = ompPrebuiltRelease.tag;
    };

    meta = {
      description = "Prebuilt omp binary patched against Nix runtime libraries";
      homepage = "https://github.com/${ompPrebuiltRelease.owner}/${ompPrebuiltRelease.repo}";
      license = pkgs.lib.licenses.mit;
      mainProgram = "omp";
      platforms = builtins.attrNames ompPrebuiltRelease.systems;
      sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
    };
  }
else
  null
