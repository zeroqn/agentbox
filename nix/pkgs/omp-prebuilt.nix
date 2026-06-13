{ pkgs, pins }:
let
  ompPrebuiltRelease = pins.ompPrebuiltRelease;
  prebuiltSystem = pkgs.stdenv.hostPlatform.system;
in
if builtins.hasAttr prebuiltSystem ompPrebuiltRelease.systems then
  let
    assetInfo = builtins.getAttr prebuiltSystem ompPrebuiltRelease.systems;
    releaseUrl =
      "https://github.com/${ompPrebuiltRelease.owner}/${ompPrebuiltRelease.repo}/releases/download/${ompPrebuiltRelease.tag}/${assetInfo.asset}";
    libcLibDir = "${pkgs.stdenv.cc.libc}/lib";
  in
  pkgs.stdenvNoCC.mkDerivation {
    pname = "omp";
    version = pkgs.lib.removePrefix "v" ompPrebuiltRelease.tag;
    src = pkgs.fetchurl {
      url = releaseUrl;
      hash = assetInfo.hash;
    };
    dontUnpack = true;

    nativeBuildInputs = [
      pkgs.binutils
    ];

    # Prevent the fixup phase from running patchelf --shrink-rpath or strip
    # on the bun-compiled binary; any ELF modification corrupts its embedded
    # payload.
    dontPatchELF = true;
    dontStrip = true;

    # autoPatchelfHook is deliberately not used here.  The upstream omp binary is
    # built with `bun build --compile`, which embeds the bun runtime and bundled
    # application code in a self-extracting ELF layout.  patchelf grows the
    # .interp section when replacing the short /lib64/ld-linux-* path with the
    # longer Nix store glibc ld-linux-* path, which shifts subsequent ELF
    # structures and corrupts the bun-embedded payload beyond the normal ELF
    # sections.  The result is a binary that bootstraps bare `bun` instead of
    # `omp`.
    #
    # Instead, install the unmodified binary and wrap it with a shim that
    # explicitly invokes the Nix glibc dynamic linker.  The dynamic linker
    # resolves needed libraries from its own store path and the binary's built-in
    # RUNPATH; the wrapper only replaces the interpreter, leaving the rest of the
    # ELF intact.

    installPhase = ''
      runHook preInstall

      readelf -h "$src" >/dev/null
      install -Dm755 "$src" "$out/libexec/omp/omp.bin"

      # Resolve the correct ld-linux-* path for this architecture.
      ld_linux=$(ls "${libcLibDir}"/ld-linux*.so.* 2>/dev/null | head -n1)
      if [ -z "$ld_linux" ]; then
        echo "omp-prebuilt: no dynamic linker found in ${libcLibDir}" >&2
        exit 1
      fi

      mkdir -p "$out/bin"
      cat > "$out/bin/omp" << WRAPPER
#!/bin/sh
exec $ld_linux "$out/libexec/omp/omp.bin" "\$@"
WRAPPER
      chmod +x "$out/bin/omp"

      runHook postInstall
    '';

    passthru = {
      inherit releaseUrl;
      releaseTag = ompPrebuiltRelease.tag;
    };

    meta = {
      description = "Prebuilt omp binary with dynamic-linker wrapper";
      homepage = "https://github.com/${ompPrebuiltRelease.owner}/${ompPrebuiltRelease.repo}";
      license = pkgs.lib.licenses.mit;
      mainProgram = "omp";
      platforms = builtins.attrNames ompPrebuiltRelease.systems;
      sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
    };
  }
else
  null
