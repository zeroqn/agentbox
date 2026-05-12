{ pkgs, pins }:

pkgs.buildNpmPackage {
  pname = "pi-coding-agent";
  version = pins.piCodingAgent.version;

  src = pkgs.fetchFromGitHub {
    owner = pins.piCodingAgent.owner;
    repo = pins.piCodingAgent.repo;
    rev = pins.piCodingAgent.rev;
    hash = pins.piCodingAgent.srcHash;
  };

  npmDepsHash = pins.piCodingAgent.npmDepsHash;
  npmDepsFetcherVersion = 2;
  npmWorkspace = "packages/coding-agent";
  npmRebuildFlags = [ "--ignore-scripts" ];

  postPatch = ''
    cp ${./pi-coding-agent-package.json} package.json
    cp ${./pi-coding-agent-package-lock.json} package-lock.json
    substituteInPlace packages/ai/package.json \
      --replace-fail 'npm run generate-models && tsgo -p tsconfig.build.json' \
                     'tsgo -p tsconfig.build.json'
  '';

  nativeBuildInputs = [ pkgs.bun ];
  npmBuildScript = "build:binary";

  installPhase = ''
    runHook preInstall

    mkdir -p $out/lib/pi-coding-agent $out/bin
    cp -R packages/coding-agent/dist/. $out/lib/pi-coding-agent/
    chmod +x $out/lib/pi-coding-agent/pi
    ln -s ../lib/pi-coding-agent/pi $out/bin/pi

    install -Dm644 packages/coding-agent/README.md $out/share/doc/pi-coding-agent/README.md
    install -Dm644 packages/coding-agent/CHANGELOG.md $out/share/doc/pi-coding-agent/CHANGELOG.md
    cp -R packages/coding-agent/docs $out/share/doc/pi-coding-agent/docs

    runHook postInstall
  '';

  nativeInstallCheckInputs = [ pkgs.versionCheckHook ];
  doInstallCheck = true;
  versionCheckProgram = "${placeholder "out"}/bin/pi";
  versionCheckProgramArg = "--version";

  passthru = {
    sourceUrl = "https://github.com/${pins.piCodingAgent.owner}/${pins.piCodingAgent.repo}/tree/${pins.piCodingAgent.rev}/packages/coding-agent";
  };

  meta = {
    description = "Minimal terminal coding harness";
    homepage = "https://github.com/${pins.piCodingAgent.owner}/${pins.piCodingAgent.repo}/tree/main/packages/coding-agent";
    license = pkgs.lib.licenses.mit;
    mainProgram = "pi";
    platforms = [
      "aarch64-linux"
      "x86_64-linux"
    ];
    sourceProvenance = [ pkgs.lib.sourceTypes.fromSource ];
  };
}
