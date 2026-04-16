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
          ohMyCodexVersion = "0.12.6";

          ohMyCodex = pkgs.buildNpmPackage {
            pname = "oh-my-codex";
            version = ohMyCodexVersion;

            src = pkgs.fetchFromGitHub {
              owner = "Yeachan-Heo";
              repo = "oh-my-codex";
              rev = "v${ohMyCodexVersion}";
              hash = "sha256-Q2Z8aITmEg+yNoRxCMMAie9nuZmLUXVhqc7Tea7zV9w=";
            };

            npmDepsHash = "sha256-HgrC4uLtZ38x6myCu8AbrghrZi4aXod0A6/b19GZ4ro=";
            npmBuildScript = "build";

            meta = {
              description = "Multi-agent orchestration layer for OpenAI Codex CLI";
              homepage = "https://github.com/Yeachan-Heo/oh-my-codex";
              license = pkgs.lib.licenses.mit;
              mainProgram = "omx";
            };
          };

          fishConfig = pkgs.writeTextDir "etc/fish/conf.d/agentbox-starship.fish" ''
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
            pkgs.codex
            pkgs.coreutils
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

          rustPackage = pkgs.rustPlatform.buildRustPackage {
            pname = "agentbox";
            version = "0.1.0";
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
            version = "0.1.0";
            src = self;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            CARGO_BUILD_TARGET = muslTarget;
          };

          baseImage = pkgs.dockerTools.pullImage {
            imageName = "ghcr.io/nixos/nix";
            imageDigest = "sha256:0b1530edf840d9af519c7f3970cafbbed68d9d9554a83cc9adc04099753117e1";
            hash = "sha256-EurCvs8HYBWXcsJFD28EFLwl2DifZmAtXyPFXv+ZK6w=";
            finalImageName = "ghcr.io/nixos/nix";
            finalImageTag = "latest";
          };

          agentboxImage = pkgs.dockerTools.buildLayeredImage {
            name = "localhost/agentbox";
            tag = "latest";
            maxLayers = 2;
            fromImage = baseImage;
            contents = imagePackages ++ [
              entrypoint
              fishConfig
              rustMuslPackage
            ];
            fakeRootCommands = ''
              mkdir -p ./workspace ./home/dev/.codex
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
              ];
            };
          };
        in
        {
          default = rustPackage;
          oh-my-codex = ohMyCodex;
          agentbox = rustPackage;
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
