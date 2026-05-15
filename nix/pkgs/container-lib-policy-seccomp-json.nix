{ pkgs, pins }:
let
  pin = pins.containerLibPolicySeccompJson;
  sourceUrl = "https://raw.githubusercontent.com/${pin.owner}/${pin.repo}/${pin.rev}/${pin.path}";
  src = pkgs.fetchurl {
    url = sourceUrl;
    hash = pin.hash;
  };
in
pkgs.stdenvNoCC.mkDerivation {
  pname = "container-lib-policy-seccomp-json";
  version = "0-${builtins.substring 0 12 pin.rev}";

  dontUnpack = true;

  installPhase = ''
    install -Dm0644 ${src} "$out/share/containers/seccomp.json"
  '';

  passthru = {
    inherit sourceUrl;
    inherit (pin) owner repo rev path;
  };

  meta = {
    description = "Pinned seccomp profile from containers/container-libs";
    homepage = "https://github.com/containers/container-libs";
    license = pkgs.lib.licenses.asl20;
    platforms = pkgs.lib.platforms.all;
  };
}
