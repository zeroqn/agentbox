{ pkgs, loftdMuslPackage, configPayloads, layers }:

let
  nixConfig = import ./nix-config.nix;
  commonEnv = [
    "HOME=/home/dev"
    "USER=dev"
    "SHELL=${pkgs.fish}/bin/fish"
    "LIBCLANG_PATH=${pkgs.libclang.lib}/lib"
    "PATH=/home/dev/.codex/bin:/home/dev/.nix-profile/bin:/nix/var/nix/profiles/default/bin:${layers.imagePath}:${loftdMuslPackage}/bin"
    "NIX_CONFIG=${nixConfig}"
    "NIX_SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
    "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
    "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=${layers.clangMoldWrapper}/bin/clang_mold_wrapper"
    "RUST_SRC_PATH=${layers.rustSourceImage}/share/rust-src"
    "RUSTC_WRAPPER=${pkgs.sccache}/bin/sccache"
    "CMAKE_C_COMPILER_LAUNCHER=${pkgs.sccache}/bin/sccache"
    "CMAKE_CXX_COMPILER_LAUNCHER=${pkgs.sccache}/bin/sccache"
  ]
  ++ montyEnv;

  # The RLM extension resolves the monty worker from MONTY_BIN first, then from
  # the @pydantic/monty platform package in node_modules, and only then from
  # PATH. Pin it to the packaged worker so a stale node_modules payload cannot
  # shadow it (worker and JS client reject a protocol-version mismatch).
  montyEnv = pkgs.lib.optionals (layers.montyPackage != null) [
    "MONTY_BIN=${layers.montyPackage}/bin/monty"
  ];

  loftdEnv = [
    "LOFTD_FISH_CONFIG_SOURCE=${configPayloads.fishConfig}/share/loftd/fish/conf.d/loftd-starship.fish"
    "LOFTD_STARSHIP_CONFIG_SOURCE=${configPayloads.starshipConfig}/share/loftd/starship.toml"
    "LOFTD_MIMALLOC_LIB=${layers.mimallocLib}"
    "LOFTD_GRAPHENE_HARDENED_MALLOC_LIB=${layers.hardenedMallocLib}"
    "LOFTD_REAL_PODMAN=${layers.realPodmanBin}"
  ];
in
{
  Entrypoint = [
    "${loftdMuslPackage}/bin/loftd-guest-init"
    "enter"
    "--"
  ];
  WorkingDir = "/workspace";
  Env = commonEnv ++ loftdEnv;
}
