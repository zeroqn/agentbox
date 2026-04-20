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
          rustPackages = import ./nix/pkgs/agentbox-rust.nix {
            inherit self pkgs pins;
          };
          prebuiltAgentbox = import ./nix/pkgs/agentbox-prebuilt.nix {
            inherit pkgs pins;
          };
          agentboxImage = import ./nix/image/container.nix {
            inherit pkgs pkgsMaster ohMyCodex;
            agentboxMuslPackage = rustPackages.agentboxMuslPackage;
          };
        in
        {
          default = rustPackages.rustPackage;
          oh-my-codex = ohMyCodex;
          agentbox = rustPackages.rustPackage;
          agentbox-prebuilt = prebuiltAgentbox;
          agentbox-musl = rustPackages.agentboxMuslPackage;
          container = agentboxImage;
        }
      );

      devShells = systems.forAllSystems ({ pkgs, ... }: {
        default = import ./nix/shell/devshell.nix {
          inherit pkgs;
        };
      });

      apps = systems.forAllSystems ({ pkgs, ... }:
        import ./nix/apps/default.nix {
          inherit self pkgs;
        }
      );
    };
}
