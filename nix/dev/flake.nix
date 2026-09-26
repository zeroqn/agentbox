{
  description = "Local submodule-aware development outputs for cang";

  inputs = {
    self.submodules = true;
    cang.url = "../..";
    nixpkgs.follows = "cang/nixpkgs";
    headless.url = "github:zeroqn/headless";
  };

  outputs =
    {
      nixpkgs,
      headless,
      ...
    }:
    let
      root = ../..;
      systems = import ../../nix/lib/systems.nix {
        inherit nixpkgs headless;
      };
      pins = import ../../nix/pins.nix;
    in
    {
      packages = systems.forAllSystems (
        { pkgs, ... }:
        let
          libkrunfw = pkgs.callPackage ../../nix/pkgs/libkrunfw.nix {
            inherit pins;
            libkrunfwSrc = root + "/deps/libkrunfw";
            useLocalSource = true;
          };
          libkrunSrc = root + "/deps/libkrun";
          libkrun = (pkgs.libkrun.override {
            inherit libkrunfw;

            withBlk = true;
            withNet = true;
            withGpu = true;
            withSound = true;
            withInput = true;
          }).overrideAttrs (_oldAttrs: {
            version = "1.19.5-cang-profile";
            src = libkrunSrc;
            cargoDeps = pkgs.rustPlatform.importCargoLock {
              lockFile = libkrunSrc + "/Cargo.lock";
            };
          });
          rustPackages = import ../../nix/pkgs/cang-rust.nix {
            self = root;
            inherit
              pkgs
              pins
              libkrun
              libkrunfw
              ;
          };
        in
        {
          default = rustPackages.rustPackage;
          cang-dev = rustPackages.rustPackage;
          virglrenderer = pkgs.virglrenderer;
        }
      );
    };
}
