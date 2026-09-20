{ pkgs, pins }:

let
  montyPrebuiltRelease = pins.montyPrebuiltRelease;
  prebuiltSystem = pkgs.stdenv.hostPlatform.system;
  supportedSystems = builtins.attrNames montyPrebuiltRelease.systems;
in
if builtins.hasAttr prebuiltSystem montyPrebuiltRelease.systems then
  let
    assetInfo = builtins.getAttr prebuiltSystem montyPrebuiltRelease.systems;
    releaseUrl = "https://registry.npmjs.org/@pydantic/monty-linux-x64-gnu/-/${assetInfo.asset}";
  in
  pkgs.stdenvNoCC.mkDerivation {
    pname = "monty";
    version = montyPrebuiltRelease.version;

    src = pkgs.fetchurl {
      url = releaseUrl;
      hash = assetInfo.hash;
    };

    # Upstream builds against plain glibc, so the worker arrives with a
    # /lib64 interpreter and dynamic glibc/libstdc++ links the image does not
    # provide. Patch the interpreter and RPATH to the store.
    nativeBuildInputs = [ pkgs.autoPatchelfHook ];
    buildInputs = [
      pkgs.glibc
      pkgs.stdenv.cc.cc.lib # libgcc_s, libstdc++
    ];

    # The tarball's single `package/` root is also the source root. It carries
    # the napi `.node` addon as well; the JS client loads that from
    # node_modules, so only the worker binary is wanted here.
    installPhase = ''
      runHook preInstall
      install -Dm755 monty "$out/bin/monty"
      runHook postInstall
    '';

    passthru = {
      inherit releaseUrl;
      releaseVersion = montyPrebuiltRelease.version;
    };

    meta = {
      description = "Prebuilt monty sandboxed Python interpreter worker fetched from the npm platform package";
      homepage = "https://github.com/pydantic/monty";
      license = pkgs.lib.licenses.mit;
      mainProgram = "monty";
      platforms = supportedSystems;
      sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
    };
  }
else
  null
