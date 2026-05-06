{ pkgs, libkrun, libkrunfw }:

let
  crunFixPasstNetRev = "ae03283599d75fa8f9c077966e240a0ec71b293d";
in
(pkgs.crun.override {
  inherit libkrun;
  withLibkrun = true;
}).overrideAttrs (oldAttrs: {
  version = "${oldAttrs.version}-zeroqn-fix-passt-net-${builtins.substring 0 7 crunFixPasstNetRev}";

  src = pkgs.fetchFromGitHub {
    owner = "zeroqn";
    repo = "crun";
    rev = crunFixPasstNetRev;
    fetchSubmodules = true;
    hash = "sha256-c5yilmolW8EZii/pVH0kYG5Pn90+mfWGvHuonkZchFw=";
  };

  nativeBuildInputs = (oldAttrs.nativeBuildInputs or [ ]) ++ [ pkgs.makeWrapper ];

  postPatch = ''
    echo ${crunFixPasstNetRev} > COMMIT
  ''
  + (oldAttrs.postPatch or "");

  postFixup = (oldAttrs.postFixup or "") + ''
    # crun's krun handler resolves symbols with dlopen/dlsym, so the upstream
    # build does not retain DT_NEEDED edges to the repo-pinned krun libraries.
    # Add explicit runtime dependencies before wrapping the binary.
    ${pkgs.patchelf}/bin/patchelf \
      --add-needed libkrun.so.1 \
      --add-needed libkrunfw.so.5 \
      --add-rpath ${pkgs.lib.getLib libkrun}/lib:${pkgs.lib.getLib libkrunfw}/lib \
      "$out/bin/crun"
    ${pkgs.patchelf}/bin/patchelf --print-needed "$out/bin/crun" | grep -Fx libkrun.so.1
    ${pkgs.patchelf}/bin/patchelf --print-needed "$out/bin/crun" | grep -Fx libkrunfw.so.5
    ${pkgs.patchelf}/bin/patchelf --print-rpath "$out/bin/crun" | grep -F ${pkgs.lib.getLib libkrun}/lib
    ${pkgs.patchelf}/bin/patchelf --print-rpath "$out/bin/crun" | grep -F ${pkgs.lib.getLib libkrunfw}/lib

    wrapProgram "$out/bin/crun" \
      --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.passt ]}
  '';
})
