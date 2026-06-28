{ pkgs, pins }:

let
  rmuxPrebuiltRelease = pins.rmuxPrebuiltRelease;
  prebuiltSystem = pkgs.stdenv.hostPlatform.system;
  supportedSystems = builtins.attrNames rmuxPrebuiltRelease.systems;
in
if builtins.hasAttr prebuiltSystem rmuxPrebuiltRelease.systems then
  let
    assetInfo = builtins.getAttr prebuiltSystem rmuxPrebuiltRelease.systems;
    releaseUrl = "https://github.com/${rmuxPrebuiltRelease.owner}/${rmuxPrebuiltRelease.repo}/releases/download/${rmuxPrebuiltRelease.tag}/${assetInfo.asset}";
  in
  pkgs.stdenv.mkDerivation {
    pname = "rmux";
    version = pkgs.lib.removePrefix "v" rmuxPrebuiltRelease.tag;

    src = pkgs.fetchurl {
      url = releaseUrl;
      hash = assetInfo.hash;
    };

    nativeBuildInputs = [
      pkgs.autoPatchelfHook
    ];

    buildInputs = [
      pkgs.stdenv.cc.cc.lib
      pkgs.stdenv.cc.libc
    ];

    dontBuild = true;

    installPhase = ''
      runHook preInstall

      mkdir -p "$out"
      cp -a ./. "$out/"
      test -x "$out/bin/rmux"
      test -x "$out/bin/rmux-daemon"

      runHook postInstall
    '';

    passthru = {
      inherit releaseUrl;
      releaseTag = rmuxPrebuiltRelease.tag;
    };

    meta = {
      description = "Prebuilt rmux terminal multiplexer fetched from a GitHub release asset";
      homepage = "https://github.com/${rmuxPrebuiltRelease.owner}/${rmuxPrebuiltRelease.repo}";
      license = with pkgs.lib.licenses; [
        asl20
        mit
      ];
      mainProgram = "rmux";
      platforms = supportedSystems;
      sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
    };
  }
else
  throw ''
    rmux-prebuilt is not pinned for ${prebuiltSystem}.
    Supported systems: ${pkgs.lib.concatStringsSep ", " supportedSystems}
  ''
