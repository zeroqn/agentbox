{ pkgs, pins }:
let
  exploreHarnessSystems = pins.ohMyCodex.exploreHarnessSystems or { };
  prebuiltSystem = pkgs.stdenv.hostPlatform.system;
  exploreHarnessAssetInfo =
    if builtins.hasAttr prebuiltSystem exploreHarnessSystems then
      builtins.getAttr prebuiltSystem exploreHarnessSystems
    else
      null;
  exploreHarnessSrc =
    if exploreHarnessAssetInfo == null then
      null
    else
      pkgs.fetchurl {
        url =
          "https://github.com/Yeachan-Heo/oh-my-codex/releases/download/v${pins.ohMyCodex.version}/${exploreHarnessAssetInfo.asset}";
        hash = exploreHarnessAssetInfo.hash;
      };
  exploreHarnessDir =
    if exploreHarnessAssetInfo == null then
      null
    else
      pkgs.lib.removeSuffix ".tar.xz" exploreHarnessAssetInfo.asset;
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

  postInstall = pkgs.lib.optionalString (exploreHarnessSrc != null) ''
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT
    tar -xJf "${exploreHarnessSrc}" -C "$tmpdir"
    install -Dm755 "$tmpdir/${exploreHarnessDir}/${exploreHarnessAssetInfo.binary}" "$out/bin/omx-explore-harness"
    wrapProgram "$out/bin/omx" --set OMX_EXPLORE_BIN "$out/bin/omx-explore-harness"
  '';

  passthru = {
    exploreHarness = if exploreHarnessSrc == null then null else {
      asset = exploreHarnessAssetInfo.asset;
      binary = exploreHarnessAssetInfo.binary;
      releaseUrl =
        "https://github.com/Yeachan-Heo/oh-my-codex/releases/download/v${pins.ohMyCodex.version}/${exploreHarnessAssetInfo.asset}";
    };
  };

  meta = {
    description = "Multi-agent orchestration layer for OpenAI Codex CLI";
    homepage = "https://github.com/Yeachan-Heo/oh-my-codex";
    license = pkgs.lib.licenses.mit;
    mainProgram = "omx";
  };
}
