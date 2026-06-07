{
  lib,
  stdenv,
  stdenvNoCC,
  fetchurl,
  pins,
  bc,
  binutils,
  bison,
  cpio,
  curl,
  elfutils,
  flex,
  gawk,
  gnugrep,
  gnumake,
  gnused,
  gnutar,
  gzip,
  ncurses,
  openssl,
  patch,
  perl,
  pkg-config,
  python3,
  util-linux,
  xz,
  zlib,
  useLocalSource ? false,
  variant ? null,
}:

assert lib.elem variant [ null ];

let
  system = stdenv.hostPlatform.system;
  release = pins.libkrunfwRelease;
  systemPins = release.systems.${system} or (throw "unsupported libkrunfw system: ${system}");

  prebuilt = stdenvNoCC.mkDerivation {
    pname = "libkrunfw";
    version = release.tag;

    src = fetchurl {
      url = "https://github.com/${release.owner}/${release.repo}/releases/download/${release.tag}/${systemPins.asset}";
      hash = systemPins.hash;
    };

    sourceRoot = ".";

    installPhase = ''
      runHook preInstall

      mkdir -p $out/lib
      if [ -d lib64 ]; then
        cp -a lib64/. $out/lib/
      else
        cp -a libkrunfw.so* $out/lib/
      fi

      runHook postInstall
    '';

    meta = {
      description = "Pinned prebuilt libkrunfw guest payload shared library for agentbox";
      homepage = "https://github.com/${release.owner}/${release.repo}";
      license = with lib.licenses; [ lgpl2Only lgpl21Only ];
      platforms = lib.attrNames release.systems;
    };
  };

  kernelVersion = "linux-6.12.91";
  kernelHardenedVersion = "v6.12.91-hardened1";

  kernelTarball = fetchurl {
    url = "https://cdn.kernel.org/pub/linux/kernel/v6.x/${kernelVersion}.tar.xz";
    hash = "sha256-D/KrnhafnxlIVXRx+7RQ0wGPjFt3yvKI4aOYJYJZeWk=";
  };

  kernelHardenedPatch = fetchurl {
    url = "https://github.com/anthraxx/linux-hardened/releases/download/${kernelHardenedVersion}/linux-hardened-${kernelHardenedVersion}.patch";
    hash = "sha256-vnx9tE/9mNXV+3W+3VJiU8j7zTvsi+sTaNBzPse3WCs=";
  };

  python = python3.withPackages (pythonPackages: [
    pythonPackages.pyelftools
  ]);

  localSource = stdenv.mkDerivation {
    pname = "libkrunfw";
    version = "${release.tag}-local";

    src = ../../deps/libkrunfw;

    nativeBuildInputs = [
      bc
      binutils
      bison
      cpio
      curl
      elfutils
      flex
      gawk
      gnugrep
      gnumake
      gnused
      gnutar
      gzip
      ncurses
      openssl
      patch
      perl
      pkg-config
      python
      util-linux
      xz
      zlib
    ];

    preBuild = ''
      mkdir -p tarballs
      ln -sf ${kernelTarball} tarballs/${kernelVersion}.tar.xz
      ln -sf ${kernelHardenedPatch} tarballs/linux-hardened-${kernelHardenedVersion}.patch
      cp config-libkrunfw_x86_64-kvm config-libkrunfw_x86_64
    '';

    makeFlags = [
      "PREFIX=${placeholder "out"}"
    ];

    installPhase = ''
      runHook preInstall

      make PREFIX=$out install

      runHook postInstall
    '';

    meta = {
      description = "Local libkrunfw guest payload shared library for agentbox";
      homepage = "https://github.com/${release.owner}/${release.repo}";
      license = with lib.licenses; [ lgpl2Only lgpl21Only ];
      platforms = [ "x86_64-linux" ];
    };
  };
in
if useLocalSource then
  if system == "x86_64-linux" then
    localSource
  else
    throw "local libkrunfw source build is only supported on x86_64-linux"
else
  prebuilt
