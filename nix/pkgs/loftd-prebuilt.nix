{ pkgs, pins, libkrun ? null, libkrunfw ? null }:
let
  agentboxVersion = pins.agentboxVersion;
  loftdPrebuiltRelease = pins.loftdPrebuiltRelease;
  prebuiltSystem = pkgs.stdenv.hostPlatform.system;
  supportedSystems = builtins.attrNames loftdPrebuiltRelease.systems;
in
if builtins.hasAttr prebuiltSystem loftdPrebuiltRelease.systems then
  let
    assetInfo = builtins.getAttr prebuiltSystem loftdPrebuiltRelease.systems;
    # The updater pins release assets named loftd-<arch>-linux-flake-locked.
    releaseUrl =
      "https://github.com/${loftdPrebuiltRelease.owner}/${loftdPrebuiltRelease.repo}/releases/download/${loftdPrebuiltRelease.tag}/${assetInfo.asset}";
    runtimeTools = [
      pkgs.buildah
      pkgs.btrfs-progs
      pkgs.fuse-overlayfs
    ];
    runtimeLibraryPath = pkgs.lib.makeLibraryPath (
      pkgs.lib.optionals (libkrun != null) [ (pkgs.lib.getLib libkrun) ]
      ++ pkgs.lib.optionals (libkrunfw != null) [ (pkgs.lib.getLib libkrunfw) ]
    );
    runtimeWrapperArgs =
      [
        "--prefix"
        "PATH"
        ":"
        (pkgs.lib.makeBinPath runtimeTools)
      ]
      ++ pkgs.lib.optionals (runtimeLibraryPath != "") [
        "--prefix"
        "LD_LIBRARY_PATH"
        ":"
        runtimeLibraryPath
      ];
  in
  pkgs.stdenvNoCC.mkDerivation {
    pname = "loftd";
    version = "${agentboxVersion}-prebuilt-${loftdPrebuiltRelease.tag}";
    src = pkgs.fetchurl {
      url = releaseUrl;
      hash = assetInfo.hash;
    };
    dontUnpack = true;

    nativeBuildInputs = [
      pkgs.binutils
      pkgs.makeWrapper
    ];

    propagatedUserEnvPkgs = runtimeTools;

    installPhase = ''
      runHook preInstall

      magic="$(dd if="$src" bs=4 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n')"
      if [ "$magic" != "7f454c46" ]; then
        echo "loftd-prebuilt expected a raw ELF payload, but ${assetInfo.asset} from ${loftdPrebuiltRelease.tag} is not ELF" >&2
        echo "Do not pin wrapper-script release assets; rerun scripts/update-loftd-prebuilt.sh after a raw-ELF sha-* release is published." >&2
        exit 1
      fi
      readelf -h "$src" >/dev/null

      install -Dm755 "$src" "$out/libexec/loftd"
      makeWrapper "$out/libexec/loftd" "$out/bin/loftd" ${pkgs.lib.escapeShellArgs runtimeWrapperArgs}

      runHook postInstall
    '';

    passthru = {
      inherit releaseUrl;
      releaseTag = loftdPrebuiltRelease.tag;
    };

    meta = {
      description = "Prebuilt dynamic loftd binary wrapped with this flake's runtime tools and libkrun libraries";
      homepage = "https://github.com/${loftdPrebuiltRelease.owner}/${loftdPrebuiltRelease.repo}";
      license = pkgs.lib.licenses.mit;
      mainProgram = "loftd";
      platforms = supportedSystems;
      sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
    };
  }
else
  throw ''
    loftd-prebuilt is not pinned for ${prebuiltSystem}.
    Current published loftd assets may still be wrapper scripts; run scripts/update-loftd-prebuilt.sh after a raw-ELF sha-* release is published.
    Supported systems: ${pkgs.lib.concatStringsSep ", " supportedSystems}
  ''
