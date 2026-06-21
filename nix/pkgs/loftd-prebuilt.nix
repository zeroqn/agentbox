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
    legacyFlakeLockedAsset = pkgs.lib.hasSuffix "-linux-flake-locked" assetInfo.asset;
  in
  if legacyFlakeLockedAsset then
    throw ''
      loftd-prebuilt is pinned to legacy asset ${assetInfo.asset} from ${loftdPrebuiltRelease.tag}.
      Legacy loftd-<arch>-linux-flake-locked assets embed release-builder /nix/store references and are intentionally unsupported.
      Publish a neutral loftd-<arch>-unknown-linux-gnu sha-* release asset, then rerun scripts/update-loftd-prebuilt.sh.
    ''
  else
    let
      releaseUrl =
        "https://github.com/${loftdPrebuiltRelease.owner}/${loftdPrebuiltRelease.repo}/releases/download/${loftdPrebuiltRelease.tag}/${assetInfo.asset}";
      runtimeTools = [
        pkgs.buildah
        pkgs.btrfs-progs
        pkgs.fuse-overlayfs
        pkgs.passt
        pkgs.util-linux
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
        pkgs.autoPatchelfHook
        pkgs.binutils
      ];

      buildInputs = [
        pkgs.stdenv.cc.cc.lib
        pkgs.stdenv.cc.libc
      ];

      propagatedUserEnvPkgs = runtimeTools;

      installPhase = ''
        runHook preInstall

        magic="$(dd if="$src" bs=4 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n')"
        if [ "$magic" != "7f454c46" ]; then
          echo "loftd-prebuilt expected a neutral raw ELF payload, but ${assetInfo.asset} from ${loftdPrebuiltRelease.tag} is not ELF" >&2
          echo "Do not pin wrapper-script release assets; rerun scripts/update-loftd-prebuilt.sh after a neutral sha-* release is published." >&2
          exit 1
        fi
        readelf -h "$src" >/dev/null

        install -Dm755 "$src" "$out/bin/loftd"
        install -Dm644 ${../../crates/loftd/assets/seccomp/default.json} "$out/share/loftd/seccomp/default.json"
        mkdir -p "$out/libexec/loftd-helpers" "$out/lib/loftd"
        ln -s ${pkgs.buildah}/bin/buildah "$out/libexec/loftd-helpers/buildah"
        ln -s ${pkgs.btrfs-progs}/bin/btrfs "$out/libexec/loftd-helpers/btrfs"
        ln -s ${pkgs.btrfs-progs}/bin/mkfs.btrfs "$out/libexec/loftd-helpers/mkfs.btrfs"
        ln -s ${pkgs.util-linux}/bin/blkid "$out/libexec/loftd-helpers/blkid"
        ln -s ${pkgs.passt}/bin/pasta "$out/libexec/loftd-helpers/pasta"
        ln -s ${pkgs.passt}/bin/passt "$out/libexec/loftd-helpers/passt"
        ${pkgs.lib.optionalString (libkrun != null) ''
          for library in ${pkgs.lib.getLib libkrun}/lib/libkrun.so*; do
            ln -s "$library" "$out/lib/loftd/$(basename "$library")"
          done
        ''}
        ${pkgs.lib.optionalString (libkrunfw != null) ''
          for library in ${pkgs.lib.getLib libkrunfw}/lib/libkrunfw.so*; do
            ln -s "$library" "$out/lib/loftd/$(basename "$library")"
          done
        ''}

        runHook postInstall
      '';

      passthru = {
        inherit releaseUrl;
        releaseTag = loftdPrebuiltRelease.tag;
      };

      meta = {
        description = "Prebuilt neutral dynamic loftd binary patched with package-relative runtime helpers";
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
    Publish a neutral loftd-<arch>-unknown-linux-gnu sha-* release asset and run scripts/update-loftd-prebuilt.sh.
    Supported systems: ${pkgs.lib.concatStringsSep ", " supportedSystems}
  ''
