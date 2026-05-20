{ pkgs, ohMyCodex, agentboxMuslPackage, configPayloads, layers }:

{
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
    "OMX_API_BIN=${ohMyCodex}/bin/omx-api"
    "OMX_RUNTIME_BINARY=${ohMyCodex}/bin/omx-runtime"
    "OMX_SPARKSHELL_BIN=${ohMyCodex}/bin/omx-sparkshell"
    "OMX_EXPLORE_BIN=${ohMyCodex}/bin/omx-explore-harness"
  ];
}
