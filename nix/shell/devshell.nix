{ pkgs }:

pkgs.mkShell {
  packages = [
    pkgs.cargo
    pkgs.clippy
    pkgs.curl
    pkgs.fish
    pkgs.fuse-overlayfs
    pkgs.jq
    pkgs.podman
    pkgs.python3
    pkgs.rustc
    pkgs.rustfmt
    pkgs.starship
  ];

  shellHook = ''
    export SHELL=${pkgs.fish}/bin/fish

    if [ -z "''${AGENTBOX_DISABLE_AUTO_FISH-}" ] && [ -t 0 ] && [ -t 1 ] && [ -z "''${AGENTBOX_IN_AUTO_FISH-}" ]; then
      export AGENTBOX_IN_AUTO_FISH=1
      exec ${pkgs.fish}/bin/fish -i -C 'starship init fish | source'
    fi
  '';
}
