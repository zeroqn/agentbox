{ self, pkgs, pins, libkrun ? null, libkrunfw ? null }:
let
  agentboxVersion = pins.agentboxVersion;
  runtimeLibraryPath = pkgs.lib.makeLibraryPath (
    pkgs.lib.optionals (libkrun != null) [ (pkgs.lib.getLib libkrun) ]
    ++ pkgs.lib.optionals (libkrunfw != null) [ (pkgs.lib.getLib libkrunfw) ]
  );
  runtimePath = pkgs.lib.makeBinPath [ pkgs.buildah ];
  runtimeWrapperArgs =
    [
      "--prefix"
      "PATH"
      ":"
      runtimePath
    ]
    ++ pkgs.lib.optionals (runtimeLibraryPath != "") [
      "--prefix"
      "LD_LIBRARY_PATH"
      ":"
      runtimeLibraryPath
    ];

  rustPackage = pkgs.rustPlatform.buildRustPackage {
    pname = "agentbox";
    version = agentboxVersion;
    src = self;

    nativeBuildInputs = [ pkgs.makeWrapper ];

    cargoLock = {
      lockFile = ../../Cargo.lock;
    };

    postInstall = ''
      wrapProgram "$out/bin/agentbox" ${pkgs.lib.escapeShellArgs runtimeWrapperArgs}
    '';
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
