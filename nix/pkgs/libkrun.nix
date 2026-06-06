{ pkgs, libkrunfw }:
let
  libkrunSrc = ../../vendor/libkrun;
in
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
    hash = "sha256-dfIe2pl957MRcY1hIv6wPPX/4He+ou+eCZLbylVeGAE=";
  };
})
