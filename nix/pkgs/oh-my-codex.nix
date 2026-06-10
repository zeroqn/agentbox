{ pkgs, pins }:
let
  nativeBinarySystems = pins.ohMyCodex.nativeBinarySystems or { };
  prebuiltSystem = pkgs.stdenv.hostPlatform.system;
  nativeBinaryInfos =
    if builtins.hasAttr prebuiltSystem nativeBinarySystems then
      builtins.getAttr prebuiltSystem nativeBinarySystems
    else
      { };
  nativeBinaries = pkgs.lib.mapAttrs (
    product: info: {
      inherit info product;
      src = pkgs.fetchurl {
        url =
          "https://github.com/Yeachan-Heo/oh-my-codex/releases/download/v${pins.ohMyCodex.version}/${info.asset}";
        hash = info.hash;
      };
      dir = pkgs.lib.removeSuffix ".tar.xz" info.asset;
    }
  ) nativeBinaryInfos;
  installNativeBinaryCommands = pkgs.lib.concatStringsSep "\n" (
    pkgs.lib.mapAttrsToList (
      product: binary: ''
        tar -xJf "${binary.src}" -C "$tmpdir"
        install -Dm755 "$tmpdir/${binary.dir}/${binary.info.binary}" "$out/bin/${product}"
      ''
    ) nativeBinaries
  );
  nativeEnvByProduct = {
    omx-api = "OMX_API_BIN";
    omx-runtime = "OMX_RUNTIME_BINARY";
    omx-sparkshell = "OMX_SPARKSHELL_BIN";
  };
  wrapNativeBinaryArgs = pkgs.lib.concatStringsSep " " (
    pkgs.lib.mapAttrsToList (
      product: _binary: ''--set ${builtins.getAttr product nativeEnvByProduct} "$out/bin/${product}"''
    ) nativeBinaries
  );
in
pkgs.buildNpmPackage {
  pname = "oh-my-codex";
  version = pins.ohMyCodex.version;

  src = pkgs.fetchFromGitHub {
    owner = "Yeachan-Heo";
    repo = "oh-my-codex";
    rev = "v${pins.ohMyCodex.version}";
    hash = pins.ohMyCodex.srcHash;
  };

  npmDepsHash = pins.ohMyCodex.npmDepsHash;
  npmBuildScript = "build";
  nativeBuildInputs = [
    pkgs.gnutar
    pkgs.makeWrapper
    pkgs.xz
  ];

  postInstall = pkgs.lib.optionalString (nativeBinaries != { }) ''
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT
    ${installNativeBinaryCommands}
    wrapProgram "$out/bin/omx" ${wrapNativeBinaryArgs}
  '';

  passthru = {
    nativeBinaries = pkgs.lib.mapAttrs (_product: binary: {
      inherit (binary.info) asset binary hash;
      releaseUrl =
        "https://github.com/Yeachan-Heo/oh-my-codex/releases/download/v${pins.ohMyCodex.version}/${binary.info.asset}";
    }) nativeBinaries;
  };

  meta = {
    description = "Multi-agent orchestration layer for OpenAI Codex CLI";
    homepage = "https://github.com/Yeachan-Heo/oh-my-codex";
    license = pkgs.lib.licenses.mit;
    mainProgram = "omx";
  };
}
