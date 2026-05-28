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
          reasonix = import ./nix/pkgs/reasonix.nix {
            inherit pkgs pins;
          };
          symposium = import ./nix/pkgs/symposium.nix {
            inherit pkgs;
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
          containerLibPolicySeccompJson = import ./nix/pkgs/container-lib-policy-seccomp-json.nix {
            inherit pkgs pins;
          };
          libkrunfw = pkgs.callPackage ./nix/pkgs/libkrunfw.nix {
            inherit pins;
          };
          libkrun = import ./nix/pkgs/libkrun.nix {
            inherit pkgs libkrunfw;
          };
          crun = import ./nix/pkgs/crun.nix {
            inherit pkgs libkrun libkrunfw;
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
              reasonix
              rtkPrebuilt
              containerLibPolicySeccompJson
              libkrun
              podman
              crun
              ;
            agentboxMuslPackage = rustPackages.agentboxMuslPackage;
          };
        in
        {
          default = rustPackages.rustPackage;
          oh-my-codex = ohMyCodex;
          opencode = opencode;
          pi-coding-agent = piCodingAgent;
          reasonix = reasonix;
          symposium = symposium;
          agentbox = rustPackages.rustPackage;
          agentbox-prebuilt = prebuiltAgentbox;
          agentbox-musl = rustPackages.agentboxMuslPackage;
          libkrunfw = libkrunfw;
          libkrun = libkrun;
          crun = crun;
          podman = podman;
          container = agentboxImage;
          container-lib-policy-seccomp-json = containerLibPolicySeccompJson;
        }
        // pkgs.lib.optionalAttrs (rtkPrebuilt != null) {
          rtk-prebuilt = rtkPrebuilt;
        }
      );

      checks = systems.forAllSystems (
        { pkgs, pkgsMaster, system, ... }:
        let
          packages = self.packages.${system};
          imageChecks = import ./nix/image/checks.nix {
            inherit pkgs pkgsMaster;
            ohMyCodex = packages.oh-my-codex;
            opencode = packages.opencode;
            piCodingAgent = packages.pi-coding-agent;
            reasonix = packages.reasonix;
            rtkPrebuilt = packages.rtk-prebuilt or null;
            containerLibPolicySeccompJson = packages.container-lib-policy-seccomp-json;
            libkrun = packages.libkrun;
            podman = packages.podman;
            crun = packages.crun;
            agentboxMuslPackage = packages.agentbox-musl;
          };
        in
        {
          container-nix-db-metadata = imageChecks.imageConfigNixDbRefs;
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
