{ nixpkgs, nixpkgsMaster }:
let
  systems = [
    "x86_64-linux"
    "aarch64-linux"
  ];

  forAllSystems =
    f:
    nixpkgs.lib.genAttrs systems (
      system:
      f {
        inherit system;
        pkgs = import nixpkgs {
          inherit system;
        };
        pkgsMaster = import nixpkgsMaster {
          inherit system;
        };
      }
    );
in
{
  inherit systems forAllSystems;
}
