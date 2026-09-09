{ pkgs, pins }:

let
  zvecGrep = pins.zvecGrep;
in
pkgs.buildNpmPackage {
  pname = "zvec-grep";
  version = zvecGrep.version;

  src = pkgs.fetchFromGitHub {
    owner = zvecGrep.owner;
    repo = zvecGrep.repo;
    rev = zvecGrep.rev;
    hash = zvecGrep.srcHash;
  };

  npmDepsHash = zvecGrep.npmDepsHash;
  npmDepsFetcherVersion = 2;
  npmRebuildFlags = [ "--ignore-scripts" ];

  nativeBuildInputs = [ pkgs.makeWrapper ];

  # The kept prebuilt .node files resolve glibc/libstdc++ through the Nix node's
  # RUNPATH, so no rpath rewriting is needed and the prebuilt ELF images must not
  # be modified.
  dontPatchELF = true;

  installPhase = ''
    runHook preInstall

    mkdir -p "$out/lib/zvec-grep"
    cp -R dist "$out/lib/zvec-grep/dist"
    cp -R node_modules "$out/lib/zvec-grep/node_modules"
    cp package.json "$out/lib/zvec-grep/package.json"

    # Upstream ships one prebuilt payload per platform. Drop the ones this
    # x86_64 glibc image can never load: the musl copies need
    # libc.musl-x86_64.so.1, and the CUDA/cross-arch llama binaries and the
    # arm64 onnxruntime cannot run in the guest.
    rm -rf \
      "$out/lib/zvec-grep/node_modules/@img/sharp-linuxmusl-x64" \
      "$out/lib/zvec-grep/node_modules/@img/sharp-libvips-linuxmusl-x64" \
      "$out/lib/zvec-grep/node_modules/@node-llama-cpp/linux-arm64" \
      "$out/lib/zvec-grep/node_modules/@node-llama-cpp/linux-armv7l" \
      "$out/lib/zvec-grep/node_modules/@node-llama-cpp/linux-x64-cuda" \
      "$out/lib/zvec-grep/node_modules/@node-llama-cpp/linux-x64-cuda-ext" \
      "$out/lib/zvec-grep/node_modules/onnxruntime-node/bin/napi-v3/linux/arm64"

    makeWrapper ${pkgs.nodejs}/bin/node "$out/bin/zg" \
      --add-flags "$out/lib/zvec-grep/dist/cli/index.js"

    runHook postInstall
  '';

  passthru = {
    sourceUrl = "https://github.com/${zvecGrep.owner}/${zvecGrep.repo}/tree/${zvecGrep.rev}";
    npmRegistryUrl = "https://www.npmjs.com/package/@zvec/zvec-grep";
  };

  meta = {
    description = "Local-first hybrid workspace search CLI (zg)";
    homepage = "https://github.com/${zvecGrep.owner}/${zvecGrep.repo}";
    license = pkgs.lib.licenses.asl20;
    mainProgram = "zg";
    # The installPhase prunes to the x86_64 glibc payload set.
    platforms = [ "x86_64-linux" ];
    sourceProvenance = [ pkgs.lib.sourceTypes.fromSource ];
  };
}
