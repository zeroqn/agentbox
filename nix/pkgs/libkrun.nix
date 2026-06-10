{
  pkgs,
  pins,
  libkrunfw,
  libkrunSrc ? pkgs.fetchFromGitHub {
    inherit (pins.libkrunSource) owner repo rev;
    hash = pins.libkrunSource.srcHash;
  },
  cargoDepsHash ? pins.libkrunSource.cargoDepsHash,
}:
(pkgs.libkrun.override {
  inherit libkrunfw;

  withBlk = true;
  withNet = true;
  withGpu = true;
  withSound = true;
  withInput = true;
}).overrideAttrs (oldAttrs: {
  version = "1.18.1-loftd-profile";
  src = libkrunSrc;
  cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
    src = libkrunSrc;
    hash = cargoDepsHash;
  };
})
