{ pkgs, pins }:

let
  src = pkgs.fetchFromGitHub {
    owner = pins.dirge.owner;
    repo = pins.dirge.repo;
    rev = pins.dirge.rev;
    hash = pins.dirge.srcHash;
  };
in
pkgs.callPackage "${src}/nix/package.nix" { inherit src; }
