{ pkgs, libkrun }:

let
  crunPasstDnsRev = "ea1685e35c47b723740e0eace7031813297d37b6";
in
(pkgs.crun.override {
  inherit libkrun;
  withLibkrun = true;
}).overrideAttrs (oldAttrs: {
  version = "${oldAttrs.version}-zeroqn-passt-dns-${builtins.substring 0 7 crunPasstDnsRev}";

  src = pkgs.fetchFromGitHub {
    owner = "zeroqn";
    repo = "crun";
    rev = crunPasstDnsRev;
    fetchSubmodules = true;
    hash = "sha256-09f1R8B+8oyL7jlcOmsV8+T7vP8C9hcQmaJ3oQdMK6A=";
  };

  nativeBuildInputs = (oldAttrs.nativeBuildInputs or [ ]) ++ [ pkgs.makeWrapper ];

  postPatch = ''
    echo ${crunPasstDnsRev} > COMMIT
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
