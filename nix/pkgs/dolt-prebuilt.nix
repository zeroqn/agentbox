{ pkgs, pins }:

let
  doltPrebuiltRelease = pins.doltPrebuiltRelease;
  prebuiltSystem = pkgs.stdenv.hostPlatform.system;
  supportedSystems = builtins.attrNames doltPrebuiltRelease.systems;
in
if builtins.hasAttr prebuiltSystem doltPrebuiltRelease.systems then
  let
    assetInfo = builtins.getAttr prebuiltSystem doltPrebuiltRelease.systems;
    releaseUrl = "https://github.com/${doltPrebuiltRelease.owner}/${doltPrebuiltRelease.repo}/releases/download/${doltPrebuiltRelease.tag}/${assetInfo.asset}";
  in
  pkgs.stdenvNoCC.mkDerivation {
    pname = "dolt";
    version = pkgs.lib.removePrefix "v" doltPrebuiltRelease.tag;

    src = pkgs.fetchurl {
      url = releaseUrl;
      hash = assetInfo.hash;
    };

    # Upstream ships a fully static Go binary, so no ELF patching is needed.
    dontUnpack = true;

    installPhase = ''
      runHook preInstall

      tmpdir="$(mktemp -d)"
      trap 'rm -rf "$tmpdir"' EXIT
      ${pkgs.gnutar}/bin/tar -xzf "$src" -C "$tmpdir"
      install -Dm755 "$tmpdir"/dolt-*/bin/dolt "$out/bin/dolt"

      runHook postInstall
    '';

    passthru = {
      inherit releaseUrl;
      releaseTag = doltPrebuiltRelease.tag;
    };

    meta = {
      description = "Prebuilt Dolt version-controlled SQL database fetched from a GitHub release asset";
      homepage = "https://github.com/${doltPrebuiltRelease.owner}/${doltPrebuiltRelease.repo}";
      license = pkgs.lib.licenses.asl20;
      mainProgram = "dolt";
      platforms = supportedSystems;
      sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
    };
  }
else
  throw ''
    dolt-prebuilt is not pinned for ${prebuiltSystem}.
    Supported systems: ${pkgs.lib.concatStringsSep ", " supportedSystems}
  ''
