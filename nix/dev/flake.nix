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
          libkrun = import ../../nix/pkgs/libkrun.nix {
            inherit pkgs pins libkrunfw;
            libkrunSrc = root + "/deps/libkrun";
          };
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
