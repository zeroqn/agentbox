{ pkgs, pkgsMaster, ohMyCodex, opencode, piCodingAgent, rtkPrebuilt, libkrun, agentboxMuslPackage, entrypoint, fishConfig }:
let
  nixBuilderGroupId = 30000;
  nixBuilderCount = 32;
  nixBuilderUsers = builtins.genList (
    index:
    let
      builderNumber = index + 1;
    in
    {
      name = "nixbld${toString builderNumber}";
      inherit builderNumber;
      uid = nixBuilderGroupId + builderNumber;
    }
  ) nixBuilderCount;
  nixBuilderGroupMembers = pkgs.lib.concatMapStringsSep "," (
    builder: builder.name
  ) nixBuilderUsers;
  nixBuilderPasswdEntries = pkgs.lib.concatMapStringsSep "\n" (
    builder:
    "${builder.name}:x:${toString builder.uid}:${toString nixBuilderGroupId}:Nix build user ${toString builder.builderNumber}:/var/empty:${pkgs.runtimeShell}"
  ) nixBuilderUsers;
  clangMoldWrapper = pkgs.writeShellScriptBin "clang_mold_wrapper" ''
    exec ${pkgs.clang}/bin/clang -fuse-ld=mold "$@"
  '';
  nixCommandCompat = pkgs.writeShellScriptBin "nix" ''
    unset LD_PRELOAD
    unset NSS_WRAPPER_PASSWD
    unset NSS_WRAPPER_GROUP
    exec ${pkgs.nix}/bin/nix "$@"
  '';
  sidecarProxyWrapper = pkgs.writeShellScriptBin "agentbox-sidecar-proxy" ''
    LISTEN_PORT="$1"
    SOCKET_PATH="$2"

    unset LD_PRELOAD NSS_WRAPPER_PASSWD NSS_WRAPPER_GROUP

    if ! command -v socat >/dev/null 2>&1; then
      echo "agentbox-sidecar-proxy: socat not found on PATH" >&2
      exit 127
    fi

    echo "agentbox-sidecar-proxy: starting socat on port $LISTEN_PORT -> $SOCKET_PATH" >&2

    while true; do
      socat "TCP-LISTEN:$LISTEN_PORT,fork,reuseaddr" "UNIX-CONNECT:$SOCKET_PATH" &
      SOCAT_PID=$!

      # Wait for socat to start listening
      for _ in $(seq 1 50); do
        if timeout 1 ${pkgs.bashInteractive}/bin/bash -c "echo >/dev/tcp/127.0.0.1/$LISTEN_PORT" 2>/dev/null; then
          break
        fi
        sleep 0.1
      done

      if ! kill -0 "$SOCAT_PID" 2>/dev/null; then
        echo "agentbox-sidecar-proxy: socat failed to start, retrying..." >&2
        sleep 0.5
        continue
      fi

      echo "agentbox-sidecar-proxy: socat listening on port $LISTEN_PORT" >&2
      wait "$SOCAT_PID"
      echo "agentbox-sidecar-proxy: socat exited, restarting..." >&2
      sleep 0.5
    done
  '';

  stableRustToolchainPackages = [
    pkgs.cargo
    clangMoldWrapper
    pkgs.clippy
    pkgs.mold
    pkgs.rust-analyzer
    pkgs.rustc
    pkgs.rustfmt
    pkgs.sccache
    pkgs.rustPlatform.rustLibSrc
  ];

  cToolchainImagePackages = [
    pkgs.clang
    pkgs.gcc
    pkgs.musl
  ];

  rustToolchainImageLayer = pkgs.buildEnv {
    name = "agentbox-rust-toolchain-layer";
    paths = stableRustToolchainPackages;
    pathsToLink = [ "/" ];
  };

  pythonToolchain = pkgs.python3.withPackages (ps: [
    ps.pip
    ps.pyyaml
    ps.tree-sitter
    ps.tree-sitter-rust
  ]);

  dynamicToolchainImagePackages = [
    pkgs.nodejs
    pythonToolchain
    pkgs.uv
  ];
  dynamicToolchainImageLayer = pkgs.buildEnv {
    name = "agentbox-dynamic-toolchain-layer";
    paths = dynamicToolchainImagePackages;
    pathsToLink = [ "/" ];
  };

  toolingImagePackages = [
    pkgs.bun
    pkgs.fzf
    pkgs.gh
    pkgs.neovim
  ]
  ++ pkgs.lib.optional (rtkPrebuilt != null) rtkPrebuilt
  ++ [
    libkrun
    pkgs.starship
  ];
  toolingImageLayer = pkgs.buildEnv {
    name = "agentbox-tooling-layer";
    paths = toolingImagePackages;
    pathsToLink = [ "/" ];
  };

  agentImagePackages = [
    pkgsMaster.codex
    opencode
    piCodingAgent
    ohMyCodex
  ];
  agentImageLayer = pkgs.buildEnv {
    name = "agentbox-agent-layer";
    paths = agentImagePackages;
    pathsToLink = [ "/" ];
  };

  baseImagePackages = [
    pkgs.bashInteractive
    pkgs.cacert
    pkgs.coreutils
    pkgs.curl
    pkgs.file
    pkgs.fish
    pkgs.ripgrep
    pkgs.socat
    sidecarProxyWrapper
    pkgs.procps
    pkgs.pkg-config
    pkgs.findutils
    pkgs.gitMinimal
    pkgs.gawk
    pkgs.gnugrep
    pkgs.gnused
    pkgs.gnutar
    pkgs.gzip
    pkgs."hostname-debian"
    pkgs.jq
    pkgs.less
    pkgs.nix
    pkgs.diffutils
    pkgs.nss_wrapper
    pkgs.tmux
    pkgs.util-linux
    pkgs.which
  ];

  usrBinEnvCompat = pkgs.runCommand "agentbox-usr-bin-env-compat" { } ''
    mkdir -p "$out/usr/bin"
    ln -s ${pkgs.coreutils}/bin/env "$out/usr/bin/env"
  '';
  binInterpreterCompat = pkgs.runCommand "agentbox-bin-interpreter-compat" { } ''
    mkdir -p "$out/bin"
    ln -s ${pkgs.bashInteractive}/bin/sh "$out/bin/sh"
    ln -s ${pkgs.bashInteractive}/bin/bash "$out/bin/bash"
    ln -s ${pythonToolchain}/bin/python "$out/bin/python"
    ln -s ${pythonToolchain}/bin/python3 "$out/bin/python3"
  '';

  imagePackages =
    baseImagePackages
    ++ cToolchainImagePackages
    ++ [
      rustToolchainImageLayer
      dynamicToolchainImageLayer
      toolingImageLayer
      agentImageLayer
    ];
  imagePath = pkgs.lib.makeBinPath ([ nixCommandCompat ] ++ imagePackages);
  agentboxImageMaxLayers = 10;
  agentboxImageStoreLayers = agentboxImageMaxLayers - 1;
  imageContents = imagePackages ++ [
    # The generated Codex hook and MCP config reference the raw
    # oh-my-codex store path directly, so keep that payload in the
    # image in addition to the /bin symlink tree from agentImageLayer.
    ohMyCodex
    usrBinEnvCompat
    binInterpreterCompat
    entrypoint
    fishConfig
    agentboxMuslPackage
    nixCommandCompat
  ];
  agentboxLayerPaths = [ (toString agentboxMuslPackage) ];
  agentLayerPaths = [ (toString agentImageLayer) ];
  toolingLayerPaths = [ (toString toolingImageLayer) ];
  cToolchainLayerPaths = builtins.map toString cToolchainImagePackages;
  rustLayerPaths = [ (toString rustToolchainImageLayer) ];
  dynamicToolchainLayerPaths = [ (toString dynamicToolchainImageLayer) ];
  agentboxImageLayeringPipeline = [
    [
      "split_paths"
      agentboxLayerPaths
    ]
    [
      "over"
      "rest"
      [
        "pipe"
        [
          [
            "split_paths"
            agentLayerPaths
          ]
          [
            "over"
            "rest"
            [
              "pipe"
              [
                [
                  "split_paths"
                  toolingLayerPaths
                ]
                [
                  "over"
                  "rest"
                  [
                    "pipe"
                    [
                      [
                        "split_paths"
                        dynamicToolchainLayerPaths
                      ]
                      [
                        "over"
                        "rest"
                        [
                          "pipe"
                          [
                            [
                              "split_paths"
                              rustLayerPaths
                            ]
                            [
                              "over"
                              "rest"
                              [
                                "pipe"
                                [
                                  [
                                    "split_paths"
                                    cToolchainLayerPaths
                                  ]
                                  [
                                    "flatten"
                                  ]
                                ]
                              ]
                            ]
                            [
                              "flatten"
                            ]
                          ]
                        ]
                      ]
                    ]
                  ]
                ]
                [
                  "flatten"
                ]
              ]
            ]
          ]
          [
            "flatten"
          ]
        ]
      ]
    ]
    [
      "flatten"
    ]
    [
      "limit_layers"
      agentboxImageStoreLayers
    ]
    [
      "reverse"
    ]
  ];
in
{
  inherit
    agentboxImageLayeringPipeline
    agentboxImageMaxLayers
    imageContents
    imagePath
    clangMoldWrapper
    nixCommandCompat
    nixBuilderGroupId
    nixBuilderGroupMembers
    nixBuilderPasswdEntries
    ;
}
