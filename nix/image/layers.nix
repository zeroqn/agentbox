{ pkgs, pkgsMaster, ohMyCodex, agentboxMuslPackage, entrypoint, fishConfig }:
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

  dynamicToolchainImagePackages = [
    pkgs.nodejs
    pkgs.python3
    pkgs.python3Packages.pip
    pkgs.python3Packages.pyyaml
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
    pkgs.starship
  ];
  toolingImageLayer = pkgs.buildEnv {
    name = "agentbox-tooling-layer";
    paths = toolingImagePackages;
    pathsToLink = [ "/" ];
  };

  codexImagePackages = [
    pkgsMaster.codex
    ohMyCodex
  ];
  codexImageLayer = pkgs.buildEnv {
    name = "agentbox-codex-layer";
    paths = codexImagePackages;
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

  imagePackages =
    baseImagePackages
    ++ cToolchainImagePackages
    ++ [
      rustToolchainImageLayer
      dynamicToolchainImageLayer
      toolingImageLayer
      codexImageLayer
    ];
  imagePath = pkgs.lib.makeBinPath imagePackages;
  agentboxImageMaxLayers = 10;
  agentboxImageStoreLayers = agentboxImageMaxLayers - 1;
  imageContents = imagePackages ++ [
    # The generated Codex hook and MCP config reference the raw
    # oh-my-codex store path directly, so keep that payload in the
    # image in addition to the /bin symlink tree from codexImageLayer.
    ohMyCodex
    entrypoint
    fishConfig
    agentboxMuslPackage
  ];
  agentboxLayerPaths = [ (toString agentboxMuslPackage) ];
  codexLayerPaths = [ (toString codexImageLayer) ];
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
            codexLayerPaths
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
    nixBuilderGroupId
    nixBuilderGroupMembers
    nixBuilderPasswdEntries
    ;
}
