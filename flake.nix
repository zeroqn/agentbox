{
  description = "Rust CLI for launching a Podman shell inside a Nix-based container";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      agentboxVersion = "0.1.0";
      agentboxPrebuiltRelease = {
        owner = "zeroqn";
        repo = "agentbox";
        # Bootstrap value; run scripts/update-agentbox-prebuilt.sh after the
        # first immutable sha-* release is published to pin this to that tag.
        tag = "sha-157f1f0fc947";
        systems = {
          x86_64-linux = {
            asset = "agentbox-x86_64-unknown-linux-musl";
            hash = "sha256-OIxKOjAuceSfHYn8H/yfdp/NQoPXgEYIGwUef1Ii8YI=";
          };
        };
      };

      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
            }
          )
        );
    in
    {
      packages = forAllSystems (
        pkgs:
        let
          ohMyCodexVersion = "0.13.1";
          prebuiltSystem = pkgs.stdenv.hostPlatform.system;
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
          nixBuilderPasswdEntries = pkgs.lib.concatMapStringsSep "\n" (
            builder:
            "${builder.name}:x:${toString builder.uid}:${toString nixBuilderGroupId}:Nix build user ${toString builder.builderNumber}:/var/empty:${pkgs.runtimeShell}"
          ) nixBuilderUsers;

          ohMyCodex = pkgs.buildNpmPackage {
            pname = "oh-my-codex";
            version = ohMyCodexVersion;

            src = pkgs.fetchFromGitHub {
              owner = "Yeachan-Heo";
              repo = "oh-my-codex";
              rev = "v${ohMyCodexVersion}";
              hash = "sha256-pXaaPWLr8V/PvKzl4a98Ws9CzF3VdJqMko0PLOxhPX4=";
            };

            npmDepsHash = "sha256-U2riv9DdA1nhaq8d6fBij/kEyl6L47tvh1Vg7i31v6U=";
            npmBuildScript = "build";

            meta = {
              description = "Multi-agent orchestration layer for OpenAI Codex CLI";
              homepage = "https://github.com/Yeachan-Heo/oh-my-codex";
              license = pkgs.lib.licenses.mit;
              mainProgram = "omx";
            };
          };

          homeSkeleton = pkgs.runCommand "agentbox-home-skeleton" { } ''
            mkdir -p "$out/home/dev/.codex"
          '';

          fishConfig = pkgs.writeTextDir "home/dev/.config/fish/conf.d/agentbox-starship.fish" ''
            if status is-interactive
                starship init fish | source
            end
          '';

          entrypoint = pkgs.writeShellScriptBin "agentbox-entrypoint" ''
            set -euo pipefail

            export USER=dev
            export HOME=/home/dev
            export SHELL=${pkgs.fish}/bin/fish
            runtime_uid="$(id -u)"
            runtime_gid="$(id -g)"

            tmpdir="$(mktemp -d)"
            cleanup() {
              rm -rf "$tmpdir"
            }
            trap cleanup EXIT

            cat /etc/passwd > "$tmpdir/passwd"
            cat /etc/group > "$tmpdir/group"
            chmod u+w "$tmpdir/passwd" "$tmpdir/group"
            printf 'dev:x:%s:%s:dev user:%s:%s\n' "$runtime_uid" "$runtime_gid" "$HOME" "$SHELL" >> "$tmpdir/passwd"
            printf 'dev:x:%s:\n' "$runtime_gid" >> "$tmpdir/group"

            export NSS_WRAPPER_PASSWD="$tmpdir/passwd"
            export NSS_WRAPPER_GROUP="$tmpdir/group"
            if [ -n "''${LD_PRELOAD-}" ]; then
              export LD_PRELOAD="${pkgs.nss_wrapper}/lib/libnss_wrapper.so:$LD_PRELOAD"
            else
              export LD_PRELOAD="${pkgs.nss_wrapper}/lib/libnss_wrapper.so"
            fi

            if [ "$#" -eq 0 ]; then
              set -- ${pkgs.fish}/bin/fish -l
            fi

            exec "$@"
          '';

          imagePackages = [
            pkgs.bashInteractive
            pkgs.cacert
            pkgs.codex
            pkgs.coreutils
            pkgs.curl
            pkgs.fish
            pkgs.neovim
            pkgs.ripgrep
            pkgs.fzf
            pkgs.gh
            pkgs.procps
            pkgs.findutils
            pkgs.gitMinimal
            pkgs.gnugrep
            pkgs.gnused
            pkgs.jq
            pkgs.nix
            pkgs.python3
            pkgs.diffutils
            pkgs.bun
            pkgs.nodejs
            pkgs.nss_wrapper
            ohMyCodex
            pkgs.starship
            pkgs.tmux
            pkgs.util-linux
          ];

          imagePath = pkgs.lib.makeBinPath imagePackages;
          imageRoot = pkgs.buildEnv {
            name = "agentbox-image-root";
            paths = imagePackages ++ [
              entrypoint
              homeSkeleton
              fishConfig
              rustMuslPackage
            ];
            pathsToLink = [ "/" ];
          };

          rustPackage = pkgs.rustPlatform.buildRustPackage {
            pname = "agentbox";
            version = agentboxVersion;
            src = self;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };
          };

          muslTarget =
            if pkgs.stdenv.hostPlatform.system == "x86_64-linux" then
              "x86_64-unknown-linux-musl"
            else if pkgs.stdenv.hostPlatform.system == "aarch64-linux" then
              "aarch64-unknown-linux-musl"
            else
              throw "agentbox-musl is only supported on Linux";

          rustMuslPackage = pkgs.pkgsStatic.rustPlatform.buildRustPackage {
            pname = "agentbox";
            version = agentboxVersion;
            src = self;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            CARGO_BUILD_TARGET = muslTarget;
          };

          prebuiltAgentbox =
            if builtins.hasAttr prebuiltSystem agentboxPrebuiltRelease.systems then
              let
                assetInfo = builtins.getAttr prebuiltSystem agentboxPrebuiltRelease.systems;
                releaseUrl =
                  "https://github.com/${agentboxPrebuiltRelease.owner}/${agentboxPrebuiltRelease.repo}/releases/download/${agentboxPrebuiltRelease.tag}/${assetInfo.asset}";
              in
              pkgs.stdenvNoCC.mkDerivation {
                pname = "agentbox";
                version = "${agentboxVersion}-prebuilt-${agentboxPrebuiltRelease.tag}";
                src = pkgs.fetchurl {
                  url = releaseUrl;
                  hash = assetInfo.hash;
                };
                dontUnpack = true;

                installPhase = ''
                  runHook preInstall
                  install -Dm755 "$src" "$out/bin/agentbox"
                  runHook postInstall
                '';

                passthru = {
                  inherit releaseUrl;
                  releaseTag = agentboxPrebuiltRelease.tag;
                };

                meta = {
                  description = "Prebuilt agentbox binary fetched from a published GitHub release asset";
                  homepage = "https://github.com/${agentboxPrebuiltRelease.owner}/${agentboxPrebuiltRelease.repo}";
                  license = pkgs.lib.licenses.mit;
                  mainProgram = "agentbox";
                  platforms = builtins.attrNames agentboxPrebuiltRelease.systems;
                  sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
                };
              }
            else
              throw ''
                agentbox-prebuilt is not pinned for ${prebuiltSystem}.
                Supported systems: ${pkgs.lib.concatStringsSep ", " (builtins.attrNames agentboxPrebuiltRelease.systems)}
              '';

          agentboxImage = pkgs.dockerTools.buildLayeredImage {
            name = "localhost/agentbox";
            tag = "latest";
            maxLayers = 2;
            contents = imageRoot;
            includeNixDB = true;
            fakeRootCommands = ''
              mkdir -p ./etc ./root ./tmp ./var/empty ./workspace
              chmod 1777 ./tmp
              if [ ! -e ./etc/passwd ]; then
                printf 'root:x:0:0:root:/root:/bin/sh\n' > ./etc/passwd
              fi
              if [ ! -e ./etc/group ]; then
                printf 'root:x:0:\n' > ./etc/group
              fi
              if ! grep -q '^nixbld:' ./etc/group; then
                printf 'nixbld:x:${toString nixBuilderGroupId}:\n' >> ./etc/group
              fi
              cat >> ./etc/passwd <<'EOF'
              ${nixBuilderPasswdEntries}
              EOF
              chown -R 1000:1000 ./home/dev ./workspace
            '';

            config = {
              Entrypoint = [ "${entrypoint}/bin/agentbox-entrypoint" ];
              WorkingDir = "/workspace";
              Env = [
                "HOME=/home/dev"
                "USER=dev"
                "PATH=/home/dev/.codex/bin:/home/dev/.nix-profile/bin:/nix/var/nix/profiles/default/bin:${imagePath}:${rustMuslPackage}/bin"
                "NIX_CONFIG=experimental-features = nix-command flakes"
                "NIX_SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
                "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              ];
            };
          };
        in
        {
          default = rustPackage;
          oh-my-codex = ohMyCodex;
          agentbox = rustPackage;
          agentbox-prebuilt = prebuiltAgentbox;
          agentbox-musl = rustMuslPackage;
          container = agentboxImage;
        }
      );

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
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
        };
      });

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.system}.agentbox}/bin/agentbox";
        };
      });
    };
}
