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
    wrapProgram "$out/bin/crun" \
      --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.passt ]}
  '';
})
