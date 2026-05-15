{ pkgs, pkgsMaster, ohMyCodex, opencode, piCodingAgent, rtkPrebuilt, libkrun, podman ? pkgs.podman, crun ? pkgs.crun, agentboxMuslPackage }:
let
  configPayloads = import ./config-payloads.nix { inherit pkgs; };
  entrypoint = import ./entrypoint.nix {
    inherit pkgs podman crun agentboxMuslPackage;
    fishConfig = configPayloads.fishConfig;
    starshipConfig = configPayloads.starshipConfig;
  };
  layers = import ./layers.nix {
    inherit pkgs pkgsMaster ohMyCodex opencode piCodingAgent rtkPrebuilt libkrun podman crun agentboxMuslPackage entrypoint;
    fishConfig = configPayloads.fishConfig;
    starshipConfig = configPayloads.starshipConfig;
  };
in
pkgs.dockerTools.buildLayeredImage {
  name = "localhost/agentbox";
  tag = "latest";
  maxLayers = layers.agentboxImageMaxLayers;
  contents = layers.imageContents;
  includeNixDB = true;
  layeringPipeline = layers.agentboxImageLayeringPipeline;
  fakeRootCommands = ''
    mkdir -p \
      ./etc \
      ./home/dev/.cache \
      ./home/dev/.codex \
      ./root \
      ./tmp \
      ./var/empty \
      ./workspace
    chmod 1777 ./tmp
    if [ ! -e ./etc/passwd ]; then
      printf 'root:x:0:0:root:/root:/bin/sh\n' > ./etc/passwd
    else
      sed -i '/^dev:/d' ./etc/passwd
    fi
    if [ ! -e ./etc/group ]; then
      printf 'root:x:0:\n' > ./etc/group
    fi
    printf '%s\n' '${layers.hardenedMallocLib}' > ./etc/ld-nix.so.preload
    chmod 0644 ./etc/ld-nix.so.preload
    if ! grep -q '^nixbld:' ./etc/group; then
      printf 'nixbld:x:${toString layers.nixBuilderGroupId}:${layers.nixBuilderGroupMembers}\n' >> ./etc/group
    fi
    cat >> ./etc/passwd <<'EOF_PASSWD'
    ${layers.nixBuilderPasswdEntries}
    EOF_PASSWD
    chown -R 1000:1000 ./home/dev ./workspace
  '';

  config = {
    Entrypoint = [ "${agentboxMuslPackage}/bin/agentbox-guest-init" "default" "enter" "--" ];
    WorkingDir = "/workspace";
    Env = [
      "HOME=/home/dev"
      "USER=dev"
      "SHELL=${pkgs.fish}/bin/fish"
      "AGENTBOX_FISH_CONFIG_SOURCE=${configPayloads.fishConfig}/share/agentbox/fish/conf.d/agentbox-starship.fish"
      "AGENTBOX_STARSHIP_CONFIG_SOURCE=${configPayloads.starshipConfig}/share/agentbox/starship.toml"
      "AGENTBOX_NSS_WRAPPER_LIB=${pkgs.nss_wrapper}/lib/libnss_wrapper.so"
      "AGENTBOX_GRAPHENE_HARDENED_MALLOC_LIB=${layers.hardenedMallocLib}"
      "LIBCLANG_PATH=${pkgs.libclang.lib}/lib"
      "PATH=/home/dev/.codex/bin:/home/dev/.nix-profile/bin:/nix/var/nix/profiles/default/bin:${layers.imagePath}:${agentboxMuslPackage}/bin"
      "NIX_CONFIG=experimental-features = nix-command flakes"
      "NIX_SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
      "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
      "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=${layers.clangMoldWrapper}/bin/clang_mold_wrapper"
      "RUST_SRC_PATH=${pkgs.rustPlatform.rustLibSrc}/lib/rustlib/src/rust/library"
      "RUSTC_WRAPPER=${pkgs.sccache}/bin/sccache"
      "CMAKE_C_COMPILER_LAUNCHER=${pkgs.sccache}/bin/sccache"
      "CMAKE_CXX_COMPILER_LAUNCHER=${pkgs.sccache}/bin/sccache"
      "OMX_EXPLORE_BIN=${ohMyCodex}/bin/omx-explore-harness"
    ];
  };
}
