{ pkgs, pins, libkrun }:
let
  dirgeSandboxPrebuiltRelease = pins.dirgeSandboxPrebuiltRelease;
  prebuiltSystem = pkgs.stdenv.hostPlatform.system;
in
if builtins.hasAttr prebuiltSystem dirgeSandboxPrebuiltRelease.systems then
  let
    assetInfo = builtins.getAttr prebuiltSystem dirgeSandboxPrebuiltRelease.systems;
    releaseUrl =
      "https://github.com/${dirgeSandboxPrebuiltRelease.owner}/${dirgeSandboxPrebuiltRelease.repo}/releases/download/${dirgeSandboxPrebuiltRelease.tag}/${assetInfo.asset}";
  in
  pkgs.stdenv.mkDerivation {
    pname = "dirge";
    version = pkgs.lib.removePrefix "v" dirgeSandboxPrebuiltRelease.tag;

    src = pkgs.fetchurl {
      url = releaseUrl;
      hash = assetInfo.hash;
    };
    dontUnpack = true;

    nativeBuildInputs = [
      pkgs.autoPatchelfHook
    ];

    buildInputs = [
      pkgs.stdenv.cc.cc.lib
      pkgs.stdenv.cc.libc
      libkrun
    ];

    installPhase = ''
      runHook preInstall
      tmpdir="$(mktemp -d)"
      trap 'rm -rf "$tmpdir"' EXIT
      ${pkgs.gnutar}/bin/tar -xzf "$src" -C "$tmpdir"
      install -Dm755 "$tmpdir/dirge" "$out/bin/dirge"
      install -Dm755 "$tmpdir/dirge-microvm-runner" "$out/bin/dirge-microvm-runner"
      runHook postInstall
    '';

    passthru = {
      inherit releaseUrl;
      releaseTag = dirgeSandboxPrebuiltRelease.tag;
    };

    meta = {
      description = "Prebuilt sandboxed dirge coding agent fetched from a published GitHub release asset";
      homepage = "https://github.com/${dirgeSandboxPrebuiltRelease.owner}/${dirgeSandboxPrebuiltRelease.repo}";
      license = pkgs.lib.licenses.gpl3Only;
      mainProgram = "dirge";
      platforms = builtins.attrNames dirgeSandboxPrebuiltRelease.systems;
      sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
    };
  }
else
  null
