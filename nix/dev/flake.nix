{
  description = "Local submodule-aware development outputs for loftd";

  inputs = {
    self.submodules = true;
    loftd.url = "../..";
    nixpkgs.follows = "loftd/nixpkgs";
    nixpkgsMaster.follows = "loftd/nixpkgsMaster";
    headless.url = "github:zeroqn/headless";
  };

  outputs =
    {
      nixpkgs,
      nixpkgsMaster,
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
            version = "1.18.1-loftd-profile";
            src = libkrunSrc;
            cargoDeps = pkgs.rustPlatform.importCargoLock {
              lockFile = libkrunSrc + "/Cargo.lock";
            };
          });
          rustPackages = import ../../nix/pkgs/loftd-rust.nix {
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
          loftd-dev = rustPackages.rustPackage;
          virglrenderer = pkgs.virglrenderer;
        }
      );
    };
}
