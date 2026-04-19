{ pkgs, pins }:
let
  agentboxVersion = pins.agentboxVersion;
  agentboxPrebuiltRelease = pins.agentboxPrebuiltRelease;
  prebuiltSystem = pkgs.stdenv.hostPlatform.system;
in
if builtins.hasAttr prebuiltSystem agentboxPrebuiltRelease.systems then
  let
    assetInfo = builtins.getAttr prebuiltSystem agentboxPrebuiltRelease.systems;
    releaseUrl =
      "https://github.com/${agentboxPrebuiltRelease.owner}/${agentboxPrebuiltRelease.repo}/releases/download/${agentboxPrebuiltRelease.tag}/${assetInfo.asset}";
  in
  pkgs.stdenvNoCC.mkDerivation {
    pname = "agentbox";
    version = "${agentboxVersion}-prebuilt-${agentboxPrebuiltRelease.tag}";
    src = pkgs.fetchurl {
      url = releaseUrl;
      hash = assetInfo.hash;
    };
    dontUnpack = true;

    installPhase = ''
      runHook preInstall
      install -Dm755 "$src" "$out/bin/agentbox"
      runHook postInstall
    '';

    passthru = {
      inherit releaseUrl;
      releaseTag = agentboxPrebuiltRelease.tag;
    };

    meta = {
      description = "Prebuilt agentbox binary fetched from a published GitHub release asset";
      homepage = "https://github.com/${agentboxPrebuiltRelease.owner}/${agentboxPrebuiltRelease.repo}";
      license = pkgs.lib.licenses.mit;
      mainProgram = "agentbox";
      platforms = builtins.attrNames agentboxPrebuiltRelease.systems;
      sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
    };
  }
else
  throw ''
    agentbox-prebuilt is not pinned for ${prebuiltSystem}.
    Supported systems: ${pkgs.lib.concatStringsSep ", " (builtins.attrNames agentboxPrebuiltRelease.systems)}
  ''
