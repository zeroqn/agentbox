{ pkgs, pins }:
let
  herdrPrebuiltRelease = pins.herdrPrebuiltRelease;
  prebuiltSystem = pkgs.stdenv.hostPlatform.system;
in
if builtins.hasAttr prebuiltSystem herdrPrebuiltRelease.systems then
  let
    assetInfo = builtins.getAttr prebuiltSystem herdrPrebuiltRelease.systems;
    releaseUrl = "https://github.com/${herdrPrebuiltRelease.owner}/${herdrPrebuiltRelease.repo}/releases/download/${herdrPrebuiltRelease.tag}/${assetInfo.asset}";
  in
  pkgs.stdenvNoCC.mkDerivation {
    pname = "herdr";
    version = pkgs.lib.removePrefix "v" herdrPrebuiltRelease.tag;
    src = pkgs.fetchurl {
      url = releaseUrl;
      hash = assetInfo.hash;
    };
    dontUnpack = true;
    # Upstream ships static-PIE ELFs; patchelf would corrupt them.
    dontPatchELF = true;

    installPhase = ''
      runHook preInstall
      install -Dm755 "$src" "$out/bin/herdr"
      runHook postInstall
    '';

    passthru = {
      inherit releaseUrl;
      releaseTag = herdrPrebuiltRelease.tag;
    };

    meta = {
      description = "Prebuilt herdr terminal workspace manager binary";
      homepage = "https://github.com/${herdrPrebuiltRelease.owner}/${herdrPrebuiltRelease.repo}";
      license = pkgs.lib.licenses.asl20;
      mainProgram = "herdr";
      platforms = builtins.attrNames herdrPrebuiltRelease.systems;
      sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
    };
  }
else
  null
