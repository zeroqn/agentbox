{ lib
, rustPlatform
, fetchgit
}:

rustPlatform.buildRustPackage {
  pname = "wl-cross-domain-proxy";
  version = "0.1.0";

  src = fetchgit {
    url = "https://codeberg.org/drakulix/wl-cross-domain-proxy.git";
    rev = "c6ce1ca89fb4d6f4f18d3aaf88324d40d4589177";
    hash = "sha256-ydyT4DFzWzhzOZR591UOgLjVQt/v6hRSNjzM3QtohlU=";
  };

  cargoHash = "sha256-k3dmxIuCQoOrn/VwauTdzuRw/XKQB6LPLgO5ql0rE7E=";

  meta = {
    description = "Wayland cross-domain proxy for virtio-gpu native contexts";
    homepage = "https://codeberg.org/drakulix/wl-cross-domain-proxy";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
  };
}
