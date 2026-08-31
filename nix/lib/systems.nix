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
                virglrenderer = prev.virglrenderer.overrideAttrs (old: {
                  patches = (old.patches or []) ++ [
                    ../pkgs/patches/virglrenderer-enum-26.patch
                  ];
                });
              }
            );
      }
    );
in
{
  inherit systems forAllSystems;
}
