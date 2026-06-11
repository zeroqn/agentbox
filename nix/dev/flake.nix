{
  description = "Local submodule-aware development outputs for agentbox";

  inputs = {
    self.submodules = true;
    agentbox.url = "../..";
    nixpkgs.follows = "agentbox/nixpkgs";
    nixpkgsMaster.follows = "agentbox/nixpkgsMaster";
  };

  outputs =
    {
      nixpkgs,
      nixpkgsMaster,
      ...
    }:
    let
      root = ../..;
      systems = import ../../nix/lib/systems.nix {
        inherit nixpkgs nixpkgsMaster;
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
          rustPackages = import ../../nix/pkgs/agentbox-rust.nix {
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
        }
      );
    };
}
