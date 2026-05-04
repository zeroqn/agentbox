{ pkgs }:
let
  libkrunSrc = pkgs.fetchFromGitHub {
    owner = "containers";
    repo = "libkrun";
    tag = "v1.18.0";
    hash = "sha256-R7q52ZwiL9JsGofLPhXVTk/eH6bEob3DoZe21PHSBrU=";
  };
in
(pkgs.libkrun.override {
  withBlk = true;
  withNet = true;
  withGpu = true;
  withSound = true;
  withInput = true;
}).overrideAttrs (oldAttrs: {
  version = "1.18.0";
  src = libkrunSrc;
  cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
    src = libkrunSrc;
    hash = "sha256-3IAEWF+XGeKnb61SUpuVHMPiX6q0FgQFN4/eOBCH80c=";
  };
})
