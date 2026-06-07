{ pkgs, libkrunfw }:
let
  libkrunSrc = ../../deps/libkrun;
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
    hash = "sha256-2ZjrOdrwnR1oaGmCZc/13LIlH3qPI7g9kBaYAEpwpSE=";
  };
})
