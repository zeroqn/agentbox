{
  description = "Rust CLI for launching direct-libkrun microVM task environments";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

    nixpkgs-unstable.url = "github:NixOS/nixpkgs/nixos-unstable";

    headless.url = "github:zeroqn/headless";
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-unstable,
      headless,

    }:
    let
      systems = import ./nix/lib/systems.nix {
        inherit nixpkgs headless;
      };
      pins = import ./nix/pins.nix;
    in
    {
      packages = systems.forAllSystems (
        { pkgs, system, ... }:
        let
          bun = (import nixpkgs-unstable { inherit system; }).bun;
          rioBin = headless.packages.${system}.rio-bin or null;
          piCodingAgent = import ./nix/pkgs/pi-coding-agent.nix {
            inherit pkgs pins;
          };
          dirgeSource = import ./nix/pkgs/dirge.nix {
            inherit pkgs pins;
          };
          dirgeCiSccache = import ./nix/pkgs/dirge.nix {
            inherit pkgs pins;
            enableCiSccache = true;
          };
          dirgePrebuilt = import ./nix/pkgs/dirge-prebuilt.nix {
            inherit pkgs pins libkrun;
          };
          dirge = if dirgePrebuilt != null then dirgePrebuilt else dirgeSource;
          herdrPrebuilt = import ./nix/pkgs/herdr-prebuilt.nix {
            inherit pkgs pins;
          };
          montyPrebuilt = import ./nix/pkgs/monty-prebuilt.nix {
            inherit pkgs pins;
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
          rtkPrebuilt = import ./nix/pkgs/rtk-prebuilt.nix {
            inherit pkgs pins;
          };
          zvecGrep = import ./nix/pkgs/zvec-grep.nix {
            inherit pkgs pins;
          };
          doltPrebuilt = import ./nix/pkgs/dolt-prebuilt.nix {
            inherit pkgs pins;
          };
          beadsPrebuilt = import ./nix/pkgs/beads-prebuilt.nix {
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
          prebuiltCang = import ./nix/pkgs/cang-prebuilt.nix {
            inherit
              pkgs
              pins
              libkrun
              libkrunfw
              ;
          };
          rustPackages = import ./nix/pkgs/cang-rust.nix {
            inherit
              self
              pkgs
              pins
              libkrun
              libkrunfw
              ;
          };
          rustPackagesCiSccache = import ./nix/pkgs/cang-rust.nix {
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
          mkImage =
            cangMuslPackage:
            import ./nix/image/container.nix {
              inherit
                pkgs

                piCodingAgent
                rioBin
                herdrPrebuilt
                montyPrebuilt
                rmuxPrebuilt
                rtkPrebuilt
                zvecGrep
                doltPrebuilt
                beadsPrebuilt
                containerLibPolicySeccompJson
                libkrun
                podman
                crun
                wl-cross-domain-proxy
                bun
                ;
              inherit cangMuslPackage;
            };
          cangImage = mkImage rustPackages.cangMuslPackage;
          cangImageCiSccache = mkImage rustPackagesCiSccache.cangMuslPackage;
        in
        {
          default = rustPackages.rustPackage;
          pi-coding-agent = piCodingAgent;
          dirge = dirge;
          dirge-ci-sccache = dirgeCiSccache;
          omp-prebuilt = ompPrebuilt;
          rmux-prebuilt = rmuxPrebuilt;
          symposium = symposium;
          cang = rustPackages.rustPackage;
          cang-ci-sccache = rustPackagesCiSccache.rustPackage;
          cang-prebuilt = prebuiltCang;
          cang-musl = rustPackages.cangMuslPackage;
          cang-musl-ci-sccache = rustPackagesCiSccache.cangMuslPackage;
          libkrunfw = libkrunfw;
          libkrun = libkrun;
          virglrenderer = pkgs.virglrenderer;
          wl-cross-domain-proxy = wl-cross-domain-proxy;
          crun = crun;
          podman = podman;
          container = cangImage;
          container-ci-sccache = cangImageCiSccache;
          container-lib-policy-seccomp-json = containerLibPolicySeccompJson;
          zvec-grep = zvecGrep;
          dolt-prebuilt = doltPrebuilt;
          beads-prebuilt = beadsPrebuilt;
        }
        // pkgs.lib.optionalAttrs (herdrPrebuilt != null) {
          herdr-prebuilt = herdrPrebuilt;
        }
        // pkgs.lib.optionalAttrs (montyPrebuilt != null) {
          monty-prebuilt = montyPrebuilt;
        }
        // pkgs.lib.optionalAttrs (rioBin != null) {
          rio-bin = rioBin;
        }
        // pkgs.lib.optionalAttrs (rtkPrebuilt != null) {
          rtk-prebuilt = rtkPrebuilt;
        }
        // pkgs.lib.optionalAttrs (dirgePrebuilt != null) {
          dirge-prebuilt = dirgePrebuilt;
        }
      );

      checks = systems.forAllSystems (
        {
          pkgs,

          system,
          ...
        }:
        let
          bun = (import nixpkgs-unstable { inherit system; }).bun;
          packages = self.packages.${system};
          cangImageChecks =
            import ./nix/image/checks.nix {
              inherit pkgs;
              bun = bun;
              piCodingAgent = packages.pi-coding-agent;
              rioBin = packages.rio-bin or null;
              herdrPrebuilt = packages.herdr-prebuilt or null;
              montyPrebuilt = packages.monty-prebuilt or null;
              rmuxPrebuilt = packages.rmux-prebuilt;
              rtkPrebuilt = packages.rtk-prebuilt or null;
              zvecGrep = packages.zvec-grep;
              doltPrebuilt = packages.dolt-prebuilt;
              beadsPrebuilt = packages.beads-prebuilt;
              containerLibPolicySeccompJson = packages.container-lib-policy-seccomp-json;
              libkrun = packages.libkrun;
              wl-cross-domain-proxy = packages.wl-cross-domain-proxy;
              podman = packages.podman;
              crun = packages.crun;
              cangMuslPackage = packages.cang-musl;
            };
        in
        {
          container-nix-db-metadata = cangImageChecks.imageConfigNixDbRefs;
          container-codex-absent = cangImageChecks.codexAbsent;
          container-omx-absent = cangImageChecks.omxAbsent;
          container-omp-absent = cangImageChecks.ompAbsent;
          container-dirge-absent = cangImageChecks.dirgeAbsent;
          container-gh-absent = cangImageChecks.ghAbsent;
          container-root-cargo-absent = cangImageChecks.rootCargoAbsent;
          container-wrapper-contracts = cangImageChecks.wrapperContracts;
          # The exported `virglrenderer` is the host-side patched build the cang
          # packages ship (libkrun links libvirglrenderer and the render-server
          # helper is symlinked from it), so downstream consumers cannot pick up
          # an unpatched vrend by consuming this output.
          virglrenderer-is-patched =
            pkgs.runCommand "virglrenderer-is-patched"
              {
                # Plain strings: comparing the two derivations must not pull
                # either virglrenderer build into this check's closure.
                patched = builtins.unsafeDiscardStringContext packages.virglrenderer.drvPath;
                plain = builtins.unsafeDiscardStringContext (import nixpkgs { inherit system; }).virglrenderer.drvPath;
              }
              ''
                if [ "$patched" = "$plain" ]; then
                  echo "packages.virglrenderer is the plain nixpkgs build; the host vrend patches in nix/lib/systems.nix are missing" >&2
                  exit 1
                fi
                touch "$out"
              '';
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
