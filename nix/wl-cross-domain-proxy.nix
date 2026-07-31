{ lib
, rustPlatform
}:

rustPlatform.buildRustPackage {
  pname = "wl-cross-domain-proxy";
  version = "0.1.0";

  src = ../deps/wl-cross-domain-proxy;

  cargoHash = "sha256-k3dmxIuCQoOrn/VwauTdzuRw/XKQB6LPLgO5ql0rE7E=";

  meta = {
    description = "Wayland cross-domain proxy for virtio-gpu native contexts";
    homepage = "https://codeberg.org/drakulix/wl-cross-domain-proxy";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
  };
}
