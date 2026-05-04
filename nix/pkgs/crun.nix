{ pkgs, libkrun }:

(pkgs.crun.override {
  inherit libkrun;
  withLibkrun = true;
}).overrideAttrs (oldAttrs: {
  nativeBuildInputs = (oldAttrs.nativeBuildInputs or [ ]) ++ [ pkgs.makeWrapper ];

  postFixup = (oldAttrs.postFixup or "") + ''
    wrapProgram "$out/bin/crun" \
      --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.passt ]}
  '';
})
