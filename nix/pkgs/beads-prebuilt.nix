{ pkgs, pins }:

let
  beadsPrebuiltRelease = pins.beadsPrebuiltRelease;
  prebuiltSystem = pkgs.stdenv.hostPlatform.system;
  supportedSystems = builtins.attrNames beadsPrebuiltRelease.systems;
in
if builtins.hasAttr prebuiltSystem beadsPrebuiltRelease.systems then
  let
    assetInfo = builtins.getAttr prebuiltSystem beadsPrebuiltRelease.systems;
    releaseUrl = "https://github.com/${beadsPrebuiltRelease.owner}/${beadsPrebuiltRelease.repo}/releases/download/${beadsPrebuiltRelease.tag}/${assetInfo.asset}";
  in
  pkgs.stdenv.mkDerivation {
    pname = "beads";
    version = pkgs.lib.removePrefix "v" beadsPrebuiltRelease.tag;

    src = pkgs.fetchurl {
      url = releaseUrl;
      hash = assetInfo.hash;
    };

    # Upstream builds `bd` with CGO, so it arrives with a /lib64 interpreter and
    # dynamic glibc/libstdc++ links that the image does not provide.
    nativeBuildInputs = [
      pkgs.autoPatchelfHook
    ];

    buildInputs = [
      pkgs.stdenv.cc.cc.lib
      pkgs.stdenv.cc.libc
    ];

    # The release tarball has no single top-level directory, so unpack it by hand.
    dontUnpack = true;

    installPhase = ''
      runHook preInstall

      tmpdir="$(mktemp -d)"
      trap 'rm -rf "$tmpdir"' EXIT
      ${pkgs.gnutar}/bin/tar -xzf "$src" -C "$tmpdir"
      install -Dm755 "$tmpdir/bd" "$out/bin/bd"

      runHook postInstall
    '';

    passthru = {
      inherit releaseUrl;
      releaseTag = beadsPrebuiltRelease.tag;
    };

    meta = {
      description = "Prebuilt beads issue tracker CLI (bd) fetched from a GitHub release asset";
      homepage = "https://github.com/${beadsPrebuiltRelease.owner}/${beadsPrebuiltRelease.repo}";
      license = pkgs.lib.licenses.mit;
      mainProgram = "bd";
      platforms = supportedSystems;
      sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
    };
  }
else
  throw ''
    beads-prebuilt is not pinned for ${prebuiltSystem}.
    Supported systems: ${pkgs.lib.concatStringsSep ", " supportedSystems}
  ''
