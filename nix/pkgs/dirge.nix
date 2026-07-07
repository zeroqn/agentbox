{ pkgs, pins, enableCiSccache ? false }:

let
  src = pkgs.fetchFromGitHub {
    owner = pins.dirge.owner;
    repo = pins.dirge.repo;
    rev = pins.dirge.rev;
    hash = pins.dirge.srcHash;
  };
  package = pkgs.callPackage "${src}/nix/package.nix" { inherit src; };
in
if enableCiSccache then
  package.overrideAttrs (old: {
    nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ pkgs.sccache ];
    RUSTC_WRAPPER = "${pkgs.sccache}/bin/sccache";
    SCCACHE_DIR = "/nix/var/cache/sccache";
    SCCACHE_IGNORE_SERVER_IO_ERROR = "1";
  })
else
  package
