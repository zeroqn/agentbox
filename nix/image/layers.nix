{
  pkgs,
  piCodingAgent,
  rioBin,
  dirge,
  ompPrebuilt,
  rmuxPrebuilt,
  rtkPrebuilt,
  containerLibPolicySeccompJson,
  libkrun,
  wl-cross-domain-proxy,
  podman ? pkgs.podman,
  crun ? pkgs.crun,
  agentboxMuslPackage,
  fishConfig,
  starshipConfig,
  imageVariant ? "agentbox",
}:
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
  nixBuilderGroupMembers = pkgs.lib.concatMapStringsSep "," (builder: builder.name) nixBuilderUsers;
  nixBuilderPasswdEntries = pkgs.lib.concatMapStringsSep "\n" (
    builder:
    "${builder.name}:x:${toString builder.uid}:${toString nixBuilderGroupId}:Nix build user ${toString builder.builderNumber}:/var/empty:${pkgs.runtimeShell}"
  ) nixBuilderUsers;
  clangMoldWrapper = pkgs.writeShellScriptBin "clang_mold_wrapper" ''
    exec ${pkgs.clang}/bin/clang -fuse-ld=mold "$@"
  '';
  grapheneHardenedMalloc = pkgs.graphene-hardened-malloc.overrideAttrs (_old: {
    version = "14";
    src = pkgs.fetchFromGitHub {
      owner = "GrapheneOS";
      repo = "hardened_malloc";
      tag = "14";
      hash = "sha256-QUGDJyTnD5MuBUMlc4PZOZSAfevVUB6QbncVyXIAgb8=";
    };
  });
  emptyLdNixSoPreload = pkgs.writeText "agentbox-empty-ld-nix-so-preload" "";
  mimallocLib = "${pkgs.mimalloc}/lib/libmimalloc.so";
  hardenedMallocLib = "${grapheneHardenedMalloc}/lib/libhardened_malloc.so";
  hardeningRun = pkgs.writeShellScriptBin "hardening-run" ''
    if [ "$#" -eq 0 ]; then
      echo "usage: hardening-run COMMAND [ARG ...]" >&2
      exit 64
    fi

    allocator_lib=${hardenedMallocLib}
    case ":''${LD_PRELOAD-}:" in
      *":$allocator_lib:"*) ;;
      "::") export LD_PRELOAD="$allocator_lib" ;;
      *) export LD_PRELOAD="$allocator_lib:$LD_PRELOAD" ;;
    esac

    exec "$@"
  '';
  rustcCommandCompat = pkgs.writeShellScriptBin "rustc" ''
    unset LD_PRELOAD NSS_WRAPPER_PASSWD NSS_WRAPPER_GROUP
    exec ${pkgs.bubblewrap}/bin/bwrap \
      --dev-bind / / \
      --ro-bind ${emptyLdNixSoPreload} /etc/ld-nix.so.preload \
      --unsetenv LD_PRELOAD \
      --unsetenv NSS_WRAPPER_PASSWD \
      --unsetenv NSS_WRAPPER_GROUP \
      -- \
      ${pkgs.rustc}/bin/rustc "$@"
  '';
  rustAnalyzerCommandCompat = pkgs.writeShellScriptBin "rust-analyzer" ''
    unset LD_PRELOAD NSS_WRAPPER_PASSWD NSS_WRAPPER_GROUP
    exec ${pkgs.bubblewrap}/bin/bwrap \
      --dev-bind / / \
      --ro-bind ${emptyLdNixSoPreload} /etc/ld-nix.so.preload \
      --unsetenv LD_PRELOAD \
      --unsetenv NSS_WRAPPER_PASSWD \
      --unsetenv NSS_WRAPPER_GROUP \
      -- \
      ${pkgs.rust-analyzer}/bin/rust-analyzer "$@"
  '';
  agentboxNixCommandCompat = pkgs.writeShellScriptBin "nix" ''
    unset LD_PRELOAD
    unset NSS_WRAPPER_PASSWD
    unset NSS_WRAPPER_GROUP
    if [ "''${AGENTBOX_LIBKRUN_NIX_OVERLAY:-}" = "1" ]; then
      export NIX_REMOTE="''${NIX_REMOTE:-unix:///nix/var/nix/daemon-socket/socket}"
      agentbox_nix_ready_marker="/tmp/agentbox-nix-daemon-ready-$(${pkgs.coreutils}/bin/id -u)"
      if [ ! -e "$agentbox_nix_ready_marker" ]; then
        ${agentboxMuslPackage}/bin/agentbox-guest-init libkrun nix wait
        ${pkgs.nix}/bin/nix store info --store "$NIX_REMOTE" --json >/dev/null
        : > "$agentbox_nix_ready_marker"
      fi
    fi
    exec ${pkgs.nix}/bin/nix "$@"
  '';
  loftdNixCommandCompat = pkgs.writeShellScriptBin "nix" ''
    unset LD_PRELOAD
    unset NSS_WRAPPER_PASSWD
    unset NSS_WRAPPER_GROUP
    if [ "''${LOFTD_NIX_OVERLAY:-}" = "1" ]; then
      export NIX_REMOTE="''${NIX_REMOTE:-unix:///nix/var/nix/daemon-socket/socket}"
      loftd_nix_ready_marker="/tmp/loftd-nix-daemon-ready-$(${pkgs.coreutils}/bin/id -u)"
      if [ ! -e "$loftd_nix_ready_marker" ]; then
        ${agentboxMuslPackage}/bin/loftd-guest-init internal nix wait
        ${pkgs.nix}/bin/nix store info --store "$NIX_REMOTE" --json >/dev/null
        : > "$loftd_nix_ready_marker"
      fi
    fi
    exec ${pkgs.nix}/bin/nix "$@"
  '';
  agentboxPodmanCommandCompat = pkgs.writeShellScriptBin "podman" ''
    unset LD_PRELOAD
    unset NSS_WRAPPER_PASSWD
    unset NSS_WRAPPER_GROUP
    if [ "''${AGENTBOX_LIBKRUN_CONTAINERS_STORAGE:-}" = "1" ]; then
      ${agentboxMuslPackage}/bin/agentbox-guest-init libkrun podman wait
    fi
    exec ${podman}/bin/podman "$@"
  '';
  loftdPodmanCommandCompat = pkgs.writeShellScriptBin "podman" ''
    unset LD_PRELOAD
    unset NSS_WRAPPER_PASSWD
    unset NSS_WRAPPER_GROUP
    if [ "''${LOFTD_CONTAINERS_STORAGE:-}" = "1" ]; then
      ${agentboxMuslPackage}/bin/loftd-guest-init internal podman wait
    fi
    exec ${podman}/bin/podman "$@"
  '';

  agentboxDockerCommandCompat = pkgs.writeShellScriptBin "docker" ''
    unset LD_PRELOAD
    unset NSS_WRAPPER_PASSWD
    unset NSS_WRAPPER_GROUP
    if [ "''${AGENTBOX_LIBKRUN_CONTAINERS_STORAGE:-}" = "1" ]; then
      ${agentboxMuslPackage}/bin/agentbox-guest-init libkrun podman service-wait
    fi
    exec ${podman}/bin/podman "$@"
  '';
  loftdDockerCommandCompat = pkgs.writeShellScriptBin "docker" ''
    unset LD_PRELOAD
    unset NSS_WRAPPER_PASSWD
    unset NSS_WRAPPER_GROUP
    if [ "''${LOFTD_CONTAINERS_STORAGE:-}" = "1" ]; then
      ${agentboxMuslPackage}/bin/loftd-guest-init internal podman service-wait
    fi
    exec ${podman}/bin/podman "$@"
  '';

  agentboxDockerComposeCommandCompat = pkgs.writeShellScriptBin "docker-compose" ''
    unset LD_PRELOAD
    unset NSS_WRAPPER_PASSWD
    unset NSS_WRAPPER_GROUP
    if [ "''${AGENTBOX_LIBKRUN_CONTAINERS_STORAGE:-}" = "1" ]; then
      ${agentboxMuslPackage}/bin/agentbox-guest-init libkrun podman service-wait
    fi
    exec ${pkgs.docker-compose}/bin/docker-compose "$@"
  '';
  loftdDockerComposeCommandCompat = pkgs.writeShellScriptBin "docker-compose" ''
    unset LD_PRELOAD
    unset NSS_WRAPPER_PASSWD
    unset NSS_WRAPPER_GROUP
    if [ "''${LOFTD_CONTAINERS_STORAGE:-}" = "1" ]; then
      ${agentboxMuslPackage}/bin/loftd-guest-init internal podman service-wait
    fi
    exec ${pkgs.docker-compose}/bin/docker-compose "$@"
  '';
  loftdAsDevCommandCompat = pkgs.writeShellScriptBin "loftd-as-dev" ''
    unset LD_PRELOAD
    unset NSS_WRAPPER_PASSWD
    unset NSS_WRAPPER_GROUP
    exec ${agentboxMuslPackage}/bin/loftd-guest-init as-dev "$@"
  '';

  commandCompat =
    if imageVariant == "loftd" then
      {
        nix = loftdNixCommandCompat;
        podman = loftdPodmanCommandCompat;
        docker = loftdDockerCommandCompat;
        dockerCompose = loftdDockerComposeCommandCompat;
        asDev = loftdAsDevCommandCompat;
      }
    else
      {
        nix = agentboxNixCommandCompat;
        podman = agentboxPodmanCommandCompat;
        docker = agentboxDockerCommandCompat;
        dockerCompose = agentboxDockerComposeCommandCompat;
        asDev = null;
      };
  loftdOnlyCommandCompat = pkgs.lib.optional (commandCompat.asDev != null) commandCompat.asDev;
  nixCommandCompat = commandCompat.nix;
  podmanCommandCompat = commandCompat.podman;
  dockerCommandCompat = commandCompat.docker;
  dockerComposeCommandCompat = commandCompat.dockerCompose;
  nixStoreDbCheck = import ./nix-store-db-check.nix { inherit pkgs imageVariant; };

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

  sidecarEntrypoint = pkgs.writeShellScriptBin "agentbox-nix-sidecar-entrypoint" ''
    set -euo pipefail

    mkdir -p /nix/var/nix/daemon-socket
    mkdir -p /nix/var/log/nix
    chmod 0755 /nix/var/nix/daemon-socket

    echo "agentbox-sidecar: starting nix-daemon"
    if ! command -v nix-daemon >/dev/null 2>&1; then
      echo "agentbox-sidecar: nix-daemon not found on PATH"
      exit 127
    fi

    unset LD_PRELOAD NSS_WRAPPER_PASSWD NSS_WRAPPER_GROUP

    echo "agentbox-sidecar: /nix/store has $(ls /nix/store 2>/dev/null | wc -l) entries"
    echo "agentbox-sidecar: /nix/var/nix/db $(if [ -d /nix/var/nix/db ]; then echo exists; else echo missing; fi)"

    nix-daemon --daemon 2>/tmp/nix-daemon-stderr.log &
    echo "agentbox-sidecar: nix-daemon spawned"
    sleep 0.5

    if [ -s /tmp/nix-daemon-stderr.log ]; then
      echo "agentbox-sidecar: nix-daemon stderr:"
      cat /tmp/nix-daemon-stderr.log >&2
    fi

    if pgrep -x nix-daemon >/dev/null 2>&1; then
      echo "agentbox-sidecar: nix-daemon process is running"
    else
      echo "agentbox-sidecar: nix-daemon process is NOT running after startup"
    fi

    attempt=0
    while [ ! -S /nix/var/nix/daemon-socket/socket ]; do
      attempt=$((attempt + 1))
      if [ "$attempt" -ge 300 ]; then
        echo "agentbox-sidecar: daemon socket not created after 30s"
        ls -ald /nix/var/nix /nix/var/nix/daemon-socket || true
        ls -al /nix/var/nix/daemon-socket || true
        ps -ef | grep -E 'nix-daemon' | grep -v grep | grep -v agentbox || true
        exit 1
      fi
      sleep 0.1
    done

    echo "agentbox-sidecar: daemon socket ready"
    echo "agentbox-sidecar: starting nix-proxy socat"
    agentbox-sidecar-proxy 19876 /nix/var/nix/daemon-socket/socket &
    exec tail -f /dev/null
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

  muslBin = pkgs.lib.getBin pkgs.musl;

  cToolchainPathPackages = [
    pkgs.clang
    pkgs.gcc
    muslBin
  ];

  cToolchainImagePackages = cToolchainPathPackages ++ [
    pkgs.libclang.lib
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
    pkgs.cargo-deny
    pkgs.fzf
    pkgs.gh
    pkgs.neovim
    pkgs.nixfmt
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
    pkgs.codex
    pkgs.bubblewrap
    piCodingAgent
    dirge
    ompPrebuilt
  ];
  agentImageLayer = pkgs.buildEnv {
    name = "agentbox-agent-layer";
    paths = agentImagePackages;
    pathsToLink = [ "/" ];
  };

  rootlessPodmanImagePackages = [
    podman
    pkgs.buildah
    crun
    pkgs.conmon
    pkgs.netavark
    pkgs.aardvark-dns
    pkgs.passt
    pkgs.shadow
    pkgs.docker-compose
  ];

  baseImagePackages = [
    pkgs.mimalloc
    grapheneHardenedMalloc
    hardeningRun
    pkgs.bashInteractive
    pkgs.btrfs-progs
    pkgs.cacert
    pkgs.coreutils
    pkgs.curl
    pkgs.openssl
    pkgs.fd
    pkgs.file
    pkgs.fish
    pkgs.ripgrep
    pkgs.socat
    sidecarProxyWrapper
    sidecarEntrypoint
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
    nixStoreDbCheck
    pkgs.diffutils
    pkgs.nss_wrapper
    pkgs.tmux
    rmuxPrebuilt
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
    ++ rootlessPodmanImagePackages
    ++ cToolchainImagePackages
    ++ [
      rustToolchainImageLayer
      dynamicToolchainImageLayer
      toolingImageLayer
      agentImageLayer
    ]
    ++ pkgs.lib.optionals (imageVariant == "loftd") (
      pkgs.lib.optional (rioBin != null) rioBin
      ++ [
        pkgs.perf
        pkgs.strace
        wl-cross-domain-proxy
      ]
    );
  imagePathPackages =
    baseImagePackages
    ++ rootlessPodmanImagePackages
    ++ cToolchainPathPackages
    ++ [
      rustToolchainImageLayer
      dynamicToolchainImageLayer
      toolingImageLayer
      agentImageLayer
    ]
    ++ pkgs.lib.optionals (imageVariant == "loftd") (
      pkgs.lib.optional (rioBin != null) rioBin
      ++ [
        pkgs.perf
        pkgs.strace
        pkgs.waypipe
        wl-cross-domain-proxy
      ]
    );
  imagePath = pkgs.lib.makeBinPath (
    [
      rustcCommandCompat
      rustAnalyzerCommandCompat
      nixCommandCompat
      podmanCommandCompat
      dockerCommandCompat
      dockerComposeCommandCompat
    ]
    ++ loftdOnlyCommandCompat
    ++ imagePathPackages
  );
  realPodmanBin = "${podman}/bin/podman";
  agentboxImageMaxLayers = 10;
  agentboxImageStoreLayers = agentboxImageMaxLayers - 1;
  imageContents =
    imagePackages
    ++ [
      usrBinEnvCompat
      binInterpreterCompat
      fishConfig
      starshipConfig
      containerLibPolicySeccompJson
      agentboxMuslPackage
      nixCommandCompat
      podmanCommandCompat
      dockerCommandCompat
      dockerComposeCommandCompat
    ]
    ++ loftdOnlyCommandCompat
    ++ [
      rustcCommandCompat
      rustAnalyzerCommandCompat
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
    agentImageLayer
    agentboxImageLayeringPipeline
    agentboxImageMaxLayers
    imageContents
    imagePath
    realPodmanBin
    clangMoldWrapper
    nixCommandCompat
    nixStoreDbCheck
    podmanCommandCompat
    dockerCommandCompat
    dockerComposeCommandCompat
    loftdAsDevCommandCompat
    rustcCommandCompat
    rustAnalyzerCommandCompat
    grapheneHardenedMalloc
    hardeningRun
    mimallocLib
    hardenedMallocLib
    nixBuilderGroupId
    nixBuilderGroupMembers
    nixBuilderPasswdEntries
    ;
}
