{
  description = "Rust CLI for launching a Podman shell inside a Nix-based container";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    nixpkgsMaster.url = "github:NixOS/nixpkgs/master";
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgsMaster,
    }:
    let
      systems = import ./nix/lib/systems.nix {
        inherit nixpkgs nixpkgsMaster;
      };
      pins = import ./nix/pins.nix;
    in
    {
      packages = systems.forAllSystems (
        { pkgs, pkgsMaster, ... }:
        let
          ohMyCodex = import ./nix/pkgs/oh-my-codex.nix {
            inherit pkgs pins;
          };
          opencode = import ./nix/pkgs/opencode.nix {
            inherit pkgs pins;
          };
          piCodingAgent = import ./nix/pkgs/pi-coding-agent.nix {
            inherit pkgs pins;
          };
          rustPackages = import ./nix/pkgs/agentbox-rust.nix {
            inherit self pkgs pins;
          };
          prebuiltAgentbox = import ./nix/pkgs/agentbox-prebuilt.nix {
            inherit pkgs pins;
          };
          rtkPrebuilt = import ./nix/pkgs/rtk-prebuilt.nix {
            inherit pkgs pins;
          };
          libkrun = import ./nix/pkgs/libkrun.nix {
            inherit pkgs;
          };
          crun = import ./nix/pkgs/crun.nix {
            inherit pkgs libkrun;
          };
          podman = pkgs.podman.override {
            inherit crun;
          };
          agentboxImage = import ./nix/image/container.nix {
            inherit
              pkgs
              pkgsMaster
              ohMyCodex
              opencode
              piCodingAgent
              rtkPrebuilt
              libkrun
              ;
            agentboxMuslPackage = rustPackages.agentboxMuslPackage;
          };
        in
        {
          default = rustPackages.rustPackage;
          oh-my-codex = ohMyCodex;
          opencode = opencode;
          pi-coding-agent = piCodingAgent;
          agentbox = rustPackages.rustPackage;
          agentbox-prebuilt = prebuiltAgentbox;
          agentbox-musl = rustPackages.agentboxMuslPackage;
          libkrun = libkrun;
          crun = crun;
          podman = podman;
          container = agentboxImage;
        }
        // pkgs.lib.optionalAttrs (rtkPrebuilt != null) {
          rtk-prebuilt = rtkPrebuilt;
        }
      );

      devShells = systems.forAllSystems (
        { pkgs, ... }:
        {
          default = import ./nix/shell/devshell.nix {
            inherit pkgs;
          };
        }
      );

      apps = systems.forAllSystems (
        { pkgs, ... }:
        import ./nix/apps/default.nix {
          inherit self pkgs;
        }
      );
    };
}
