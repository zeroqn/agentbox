{
  pkgs,
  pins,
  libkrunfw,
}:

let
  lib = pkgs.lib;
  system = pkgs.stdenv.hostPlatform.system;
  release = pins.libkrunRelease;
  libkrunSrc = builtins.fetchGit {
    # Absolute path: the flake self-source does not carry submodule content,
    # and the submodule's `.git` is an indirection file (gitdir: ../.git/modules/...)
    # that Nix only accepts as a shallow repo.
    url = "file:///home/dev/loftd/agentbox/deps/libkrun";
    # deps/libkrun submodule (loftd fork), committed with the
    # krun_set_gpu_options3 render-server fd plumbing.
    rev = "47d7bf26106d76e748576228ba9c8dd7f47d1a91";
    shallow = true;
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "libkrun";
  version = "${release.tag}-loftd-source";

  src = libkrunSrc;

  cargoLock = {
    lockFile = "${libkrunSrc}/Cargo.lock";
  };

  # Build only the libkrun cdylib with the loftd full-feature set (mirrors
  # `make BLK=1 NET=1 GPU=1 INPUT=1` from the loftd fork's prebuilt CI).
  cargoBuildFlags = [
    "-p"
    "libkrun"
    "--features"
    "gpu,net,blk,input"
  ];

  doCheck = false;

  nativeBuildInputs = [
    pkgs.pkg-config
    pkgs.clang
    pkgs.llvmPackages.libclang
  ];

  buildInputs = [
    (lib.getLib libkrunfw)
    pkgs.libcap_ng
    pkgs.libepoxy
    pkgs.libdrm
    pkgs.virglrenderer
    pkgs.pipewire
  ];

  # init-blob compiles init/init.c with `-static`; the static libc must not be a
  # global buildInput (it would shadow the dynamic libc for every Rust link), so
  # scope it to that single C compile via CC_LINUX (honored over CC).
  CC_LINUX = "${pkgs.stdenv.cc}/bin/cc -L${pkgs.glibc.static}/lib";

  # bindgen (used by krun-display/krun-input) needs libclang and the C headers.
  # libclang is multi-output: the .so and clang headers live in the `lib` output.
  LIBCLANG_PATH = "${lib.getLib pkgs.llvmPackages.libclang}/lib";
  BINDGEN_EXTRA_CLANG_ARGS = "-I${pkgs.glibc.dev}/include -I${lib.getLib pkgs.llvmPackages.libclang}/lib/clang/${lib.versions.major pkgs.llvmPackages.libclang.version}/include";

  runtimeDependencies = [
    (lib.getLib libkrunfw)
  ];

  installPhase = ''
    runHook preInstall

    mkdir -p "$out/lib" "$out/include" "$out/lib/pkgconfig"

    # buildRustPackage passes --target <triple> (stdenv.hostPlatform.config),
    # so release artifacts land in target/<triple>/release/ not target/release/.
    install -m 755 "target/${pkgs.stdenv.hostPlatform.config}/release/libkrun.so" "$out/lib/"
    ln -sf libkrun.so "$out/lib/libkrun.so.1"
    ln -sf libkrun.so "$out/lib/libkrun.so.1.18"

    install -m 644 include/libkrun.h "$out/include/"
    install -m 644 include/libkrun_display.h "$out/include/"
    install -m 644 include/libkrun_input.h "$out/include/"

    cat > "$out/lib/pkgconfig/libkrun.pc" <<PC_EOF
prefix=$out
exec_prefix=''${prefix}
libdir=''${prefix}/lib
includedir=''${prefix}/include

Name: libkrun
Description: Dynamic library for creating microVM-based process sandboxes
Version: ${release.tag}
Libs: -L''${libdir} -lkrun
Cflags: -I''${includedir}
PC_EOF

    runHook postInstall
  '';

  meta = {
    description = "loftd libkrun built from the deps/libkrun submodule (with krun_set_gpu_options3 render-server fd plumbing)";
    homepage = "https://github.com/${release.owner}/${release.repo}";
    license = with lib.licenses; [ asl20 ];
    platforms = lib.attrNames release.systems;
  };
}
