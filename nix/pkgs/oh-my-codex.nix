{ pkgs, pins }:

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

  meta = {
    description = "Multi-agent orchestration layer for OpenAI Codex CLI";
    homepage = "https://github.com/Yeachan-Heo/oh-my-codex";
    license = pkgs.lib.licenses.mit;
    mainProgram = "omx";
  };
}
