{ nixpkgs, headless }:
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
        pkgs =
          (import nixpkgs {
            inherit system;
          }).extend
            (
              final: prev: {
                mesa = headless.packages.${system}.mesa or prev.mesa;
              }
            );
      }
    );
in
{
  inherit systems forAllSystems;
}
