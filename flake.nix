{
  description = "Rust CLI for launching a Podman shell inside a Nix-based container";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  };

  outputs =
    {
      self,
      nixpkgs,

    }:
    let
      systems = import ./nix/lib/systems.nix {
        inherit nixpkgs;
      };
      pins = import ./nix/pins.nix;
    in
    {
      packages = systems.forAllSystems (
        { pkgs, ... }:
        let
          ohMyCodex = import ./nix/pkgs/oh-my-codex.nix {
            inherit pkgs pins;
          };
          piCodingAgent = import ./nix/pkgs/pi-coding-agent.nix {
            inherit pkgs pins;
          };
          dirge = import ./nix/pkgs/dirge.nix {
            inherit pkgs pins;
          };
          dirgeCiSccache = import ./nix/pkgs/dirge.nix {
            inherit pkgs pins;
            enableCiSccache = true;
          };
          ompPrebuilt = import ./nix/pkgs/omp-prebuilt.nix {
            inherit pkgs pins;
          };
          rmuxPrebuilt = import ./nix/pkgs/rmux-prebuilt.nix {
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
          libkrun = import ./nix/pkgs/libkrun.nix {
            inherit pkgs pins libkrunfw;
          };
          wl-cross-domain-proxy = pkgs.callPackage ./nix/wl-cross-domain-proxy.nix { };
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
          rustPackagesCiSccache = import ./nix/pkgs/agentbox-rust.nix {
            inherit
              self
              pkgs
              pins
              libkrun
              libkrunfw
              ;
            enableCiSccache = true;
          };
          crun = import ./nix/pkgs/crun.nix {
            inherit pkgs libkrun libkrunfw;
          };
          podman = pkgs.podman.override {
            inherit crun;
          };
          mkImageWith =
            {
              imageVariant,
              dirgePackage,
              agentboxMuslPackage,
            }:
            import ./nix/image/container.nix {
              inherit
                pkgs

                ohMyCodex
                piCodingAgent
                ompPrebuilt
                rmuxPrebuilt
                rtkPrebuilt
                containerLibPolicySeccompJson
                libkrun
                podman
                crun
                wl-cross-domain-proxy
                imageVariant
                ;
              dirge = dirgePackage;
              inherit agentboxMuslPackage;
            };
          mkImage =
            imageVariant:
            mkImageWith {
              inherit imageVariant;
              dirgePackage = dirge;
              agentboxMuslPackage = rustPackages.agentboxMuslPackage;
            };
          mkImageCiSccache =
            imageVariant:
            mkImageWith {
              inherit imageVariant;
              dirgePackage = dirgeCiSccache;
              agentboxMuslPackage = rustPackagesCiSccache.agentboxMuslPackage;
            };
          loftdImage = mkImage "loftd";
          agentboxImage = mkImage "agentbox";
          loftdImageCiSccache = mkImageCiSccache "loftd";
          agentboxImageCiSccache = mkImageCiSccache "agentbox";
        in
        {
          default = rustPackages.rustPackage;
          oh-my-codex = ohMyCodex;
          pi-coding-agent = piCodingAgent;
          dirge = dirge;
          dirge-ci-sccache = dirgeCiSccache;
          omp-prebuilt = ompPrebuilt;
          rmux-prebuilt = rmuxPrebuilt;
          symposium = symposium;
          agentbox = rustPackages.rustPackage;
          agentbox-ci-sccache = rustPackagesCiSccache.rustPackage;
          loftd = rustPackages.rustPackage;
          loftd-ci-sccache = rustPackagesCiSccache.rustPackage;
          agentbox-prebuilt = prebuiltAgentbox;
          loftd-prebuilt = prebuiltLoftd;
          agentbox-musl = rustPackages.agentboxMuslPackage;
          agentbox-musl-ci-sccache = rustPackagesCiSccache.agentboxMuslPackage;
          agentbox-container = agentboxImage;
          agentbox-container-ci-sccache = agentboxImageCiSccache;
          libkrunfw = libkrunfw;
          libkrun = libkrun;
          wl-cross-domain-proxy = wl-cross-domain-proxy;
          crun = crun;
          podman = podman;
          container = loftdImage;
          container-ci-sccache = loftdImageCiSccache;
          container-lib-policy-seccomp-json = containerLibPolicySeccompJson;
        }
        // pkgs.lib.optionalAttrs (rtkPrebuilt != null) {
          rtk-prebuilt = rtkPrebuilt;
        }
      );

      checks = systems.forAllSystems (
        {
          pkgs,

          system,
          ...
        }:
        let
          packages = self.packages.${system};
          mkImageChecks =
            imageVariant:
            import ./nix/image/checks.nix {
              inherit pkgs imageVariant;
              ohMyCodex = packages.oh-my-codex;
              piCodingAgent = packages.pi-coding-agent;
              dirge = packages.dirge;
              ompPrebuilt = packages.omp-prebuilt;
              rmuxPrebuilt = packages.rmux-prebuilt;
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
