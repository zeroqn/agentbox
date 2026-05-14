{ self, pkgs, pins }:
let
  agentboxVersion = pins.agentboxVersion;

  rustPackage = pkgs.rustPlatform.buildRustPackage {
    pname = "agentbox";
    version = agentboxVersion;
    src = self;

    cargoLock = {
      lockFile = ../../Cargo.lock;
    };
  };

  muslTarget =
    if pkgs.stdenv.hostPlatform.system == "x86_64-linux" then
      "x86_64-unknown-linux-musl"
    else if pkgs.stdenv.hostPlatform.system == "aarch64-linux" then
      "aarch64-unknown-linux-musl"
    else
      throw "agentbox-musl is only supported on Linux";

  agentboxMuslPackage = pkgs.pkgsStatic.rustPlatform.buildRustPackage {
    pname = "agentbox";
    version = agentboxVersion;
    src = self;

    cargoLock = {
      lockFile = ../../Cargo.lock;
    };

    CARGO_BUILD_TARGET = muslTarget;
  };
in
{
  inherit rustPackage agentboxMuslPackage;
}
