{ pkgs, libkrun }:

let
  crunDebugRev = "cacf90ccd68e5ad98d3c6f92efd4fd3922510b0b";
in
(pkgs.crun.override {
  inherit libkrun;
  withLibkrun = true;
}).overrideAttrs (oldAttrs: {
  version = "${oldAttrs.version}-zeroqn-debug-${builtins.substring 0 7 crunDebugRev}";

  src = pkgs.fetchFromGitHub {
    owner = "zeroqn";
    repo = "crun";
    rev = crunDebugRev;
    fetchSubmodules = true;
    hash = "sha256-jzwv3L/5YvkciXaDi8D2Qj08gFezWGbob7HRZudN46I=";
  };

  nativeBuildInputs = (oldAttrs.nativeBuildInputs or [ ]) ++ [ pkgs.makeWrapper ];

  postPatch = ''
    echo ${crunDebugRev} > COMMIT
  ''
  + (oldAttrs.postPatch or "");

  postFixup = (oldAttrs.postFixup or "") + ''
    # crun's krun handler resolves symbols with dlopen/dlsym, so the upstream
    # build does not retain a DT_NEEDED edge to libkrun. Add an explicit
    # runtime dependency on the repo-pinned libkrun before wrapping the binary.
    ${pkgs.patchelf}/bin/patchelf \
      --add-needed libkrun.so.1 \
      --add-rpath ${pkgs.lib.getLib libkrun}/lib \
      "$out/bin/crun"
    ${pkgs.patchelf}/bin/patchelf --print-needed "$out/bin/crun" | grep -Fx libkrun.so.1
    ${pkgs.patchelf}/bin/patchelf --print-rpath "$out/bin/crun" | grep -F ${pkgs.lib.getLib libkrun}/lib

    wrapProgram "$out/bin/crun" \
      --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.passt ]}
  '';
})
