{ pkgs, pins }:
let
  rtkPrebuiltRelease = pins.rtkPrebuiltRelease;
  prebuiltSystem = pkgs.stdenv.hostPlatform.system;
in
if builtins.hasAttr prebuiltSystem rtkPrebuiltRelease.systems then
  let
    assetInfo = builtins.getAttr prebuiltSystem rtkPrebuiltRelease.systems;
    releaseUrl =
      "https://github.com/${rtkPrebuiltRelease.owner}/${rtkPrebuiltRelease.repo}/releases/download/${rtkPrebuiltRelease.tag}/${assetInfo.asset}";
  in
  pkgs.stdenvNoCC.mkDerivation {
    pname = "rtk";
    version = pkgs.lib.removePrefix "v" rtkPrebuiltRelease.tag;
    src = pkgs.fetchurl {
      url = releaseUrl;
      hash = assetInfo.hash;
    };
    dontUnpack = true;

    installPhase = ''
      runHook preInstall
      tmpdir="$(mktemp -d)"
      trap 'rm -rf "$tmpdir"' EXIT
      ${pkgs.gnutar}/bin/tar -xzf "$src" -C "$tmpdir"
      install -Dm755 "$tmpdir/${assetInfo.binary}" "$out/bin/rtk"
      runHook postInstall
    '';

    passthru = {
      inherit releaseUrl;
      releaseTag = rtkPrebuiltRelease.tag;
    };

    meta = {
      description = "Prebuilt RTK binary fetched from a published GitHub release asset";
      homepage = "https://github.com/${rtkPrebuiltRelease.owner}/${rtkPrebuiltRelease.repo}";
      license = pkgs.lib.licenses.asl20;
      mainProgram = "rtk";
      platforms = builtins.attrNames rtkPrebuiltRelease.systems;
      sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
    };
  }
else
  null
