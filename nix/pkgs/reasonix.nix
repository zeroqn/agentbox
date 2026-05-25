{ pkgs, pins }:

pkgs.buildNpmPackage {
  pname = "reasonix";
  version = pins.reasonix.version;

  src = pkgs.fetchFromGitHub {
    owner = pins.reasonix.owner;
    repo = pins.reasonix.repo;
    rev = pins.reasonix.rev;
    hash = pins.reasonix.srcHash;
  };

  npmDepsHash = pins.reasonix.npmDepsHash;
  npmDepsFetcherVersion = 2;
  npmRebuildFlags = [ "--ignore-scripts" ];

  nativeBuildInputs = [
    pkgs.makeBinaryWrapper
  ];

  postPatch = ''
    substituteInPlace package.json \
      --replace-fail 'npm run build:dashboard && tsup && node scripts/copy-dashboard-vendor-css.mjs && node scripts/copy-tree-sitter-grammars.mjs' \
                     'tsup && node scripts/copy-tree-sitter-grammars.mjs'
  '';

  npmBuildScript = "build";

  postBuild = ''
    printf '%s\n' '{"type":"module"}' > dist/cli/package.json
  '';

  installPhase = ''
    runHook preInstall

    mkdir -p $out/lib/reasonix $out/bin $out/share/doc/reasonix
    cp -R dist package.json $out/lib/reasonix/
    cp README.md CHANGELOG.md LICENSE $out/share/doc/reasonix/

    makeWrapper ${pkgs.nodejs}/bin/node $out/bin/reasonix \
      --add-flags "$out/lib/reasonix/dist/cli/index.js"
    makeWrapper ${pkgs.nodejs}/bin/node $out/bin/dsnix \
      --add-flags "$out/lib/reasonix/dist/cli/index.js"

    runHook postInstall
  '';

  nativeInstallCheckInputs = [ pkgs.versionCheckHook ];
  doInstallCheck = true;
  versionCheckProgram = "${placeholder "out"}/bin/reasonix";
  versionCheckProgramArg = "--version";

  passthru = {
    sourceUrl = "https://github.com/${pins.reasonix.owner}/${pins.reasonix.repo}/tree/${pins.reasonix.rev}";
  };

  meta = {
    description = "DeepSeek-powered terminal coding assistant";
    homepage = "https://github.com/${pins.reasonix.owner}/${pins.reasonix.repo}";
    license = pkgs.lib.licenses.mit;
    mainProgram = "reasonix";
    platforms = [
      "aarch64-linux"
      "x86_64-linux"
    ];
    sourceProvenance = [ pkgs.lib.sourceTypes.fromSource ];
  };
}
