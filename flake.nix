{
  description = "Rust CLI for launching a Podman shell inside a Nix-based container";

  inputs = {
    self.submodules = true;
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
          symposium = import ./nix/pkgs/symposium.nix {
            inherit pkgs;
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
          libkrunfwDev = pkgs.callPackage ./nix/pkgs/libkrunfw.nix {
            inherit pins;
            useLocalSource = true;
          };
          libkrun = import ./nix/pkgs/libkrun.nix {
            inherit pkgs libkrunfw;
          };
          libkrunDev = import ./nix/pkgs/libkrun.nix {
            inherit pkgs;
            libkrunfw = libkrunfwDev;
          };
          prebuiltLoftd = import ./nix/pkgs/loftd-prebuilt.nix {
            inherit
              pkgs
              pins
              libkrun
              libkrunfw
              ;
          };
          rustPackages = import ./nix/pkgs/agentbox-rust.nix {
            inherit
              self
              pkgs
              pins
              libkrun
              libkrunfw
              ;
          };
          rustPackagesDev = import ./nix/pkgs/agentbox-rust.nix {
            inherit
              self
              pkgs
              pins
              ;
            libkrun = libkrunDev;
            libkrunfw = libkrunfwDev;
          };
          crun = import ./nix/pkgs/crun.nix {
            inherit pkgs libkrun libkrunfw;
          };
          podman = pkgs.podman.override {
            inherit crun;
          };
          mkImage = imageVariant: import ./nix/image/container.nix {
            inherit
              pkgs
              pkgsMaster
              ohMyCodex
              opencode
              piCodingAgent
              rtkPrebuilt
              containerLibPolicySeccompJson
              libkrun
              podman
              crun
              imageVariant
              ;
            agentboxMuslPackage = rustPackages.agentboxMuslPackage;
          };
          loftdImage = mkImage "loftd";
          agentboxImage = mkImage "agentbox";
        in
        {
          default = rustPackages.rustPackage;
          oh-my-codex = ohMyCodex;
          opencode = opencode;
          pi-coding-agent = piCodingAgent;
          symposium = symposium;
          agentbox = rustPackages.rustPackage;
          loftd = rustPackages.rustPackage;
          loftd-dev = rustPackagesDev.rustPackage;
          agentbox-prebuilt = prebuiltAgentbox;
          loftd-prebuilt = prebuiltLoftd;
          agentbox-musl = rustPackages.agentboxMuslPackage;
          agentbox-container = agentboxImage;
          libkrunfw = libkrunfw;
          libkrunfw-dev = libkrunfwDev;
          libkrun = libkrun;
          libkrun-dev = libkrunDev;
          crun = crun;
          podman = podman;
          container = loftdImage;
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
          mkImageChecks = imageVariant: import ./nix/image/checks.nix {
            inherit pkgs pkgsMaster imageVariant;
            ohMyCodex = packages.oh-my-codex;
            opencode = packages.opencode;
            piCodingAgent = packages.pi-coding-agent;
            rtkPrebuilt = packages.rtk-prebuilt or null;
            containerLibPolicySeccompJson = packages.container-lib-policy-seccomp-json;
            libkrun = packages.libkrun;
            podman = packages.podman;
            crun = packages.crun;
            agentboxMuslPackage = packages.agentbox-musl;
          };
          loftdImageChecks = mkImageChecks "loftd";
          agentboxImageChecks = mkImageChecks "agentbox";
        in
        {
          container-nix-db-metadata = loftdImageChecks.imageConfigNixDbRefs;
          container-wrapper-contracts = loftdImageChecks.wrapperContracts;
          agentbox-container-nix-db-metadata = agentboxImageChecks.imageConfigNixDbRefs;
          agentbox-container-wrapper-contracts = agentboxImageChecks.wrapperContracts;
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
