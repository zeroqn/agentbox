{
  description = "In-guest virgl venus probe: boot a bare libkrun VM with a venus virtio-gpu and check the guest sees a Vulkan device";

  inputs = {
    # The parent repo: provides the exact libkrun the product links (and, via its
    # nixpkgs, the matching virglrenderer render server for RENDER_SERVER mode).
    repo.url = "path:../..";
  };

  outputs =
    { self, repo, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = repo.inputs.nixpkgs.lib.genAttrs systems;

      mkGuestProbe =
        system:
        let
          pkgs = repo.inputs.nixpkgs.legacyPackages.${system};
        in
        pkgs.stdenv.mkDerivation {
          pname = "virgl-guest-probe";
          version = "0.1.0";
          # Only the probe source, not the whole repo tree (the repo has 100k+
          # files under target/ -- walking them for src causes fd exhaustion).
          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = ./guest-probe.c;
          };

          nativeBuildInputs = [ pkgs.gcc ];
          # vulkan-headers only for the Abi/struct layouts; the probe dlopens
          # libvulkan.so.1 at runtime, so libvulkan is deliberately NOT linked.
          buildInputs = [ pkgs.vulkan-headers ];

          buildPhase = ''
            runHook preBuild
            gcc -O2 -Wall -Wno-deprecated-declarations guest-probe.c \
              -I${pkgs.vulkan-headers}/include -ldl \
              -o guest-probe-bin
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p "$out/bin"
            cp guest-probe-bin "$out/bin/guest-probe"
            runHook postInstall
          '';

          meta.mainProgram = "guest-probe";
        };

      mkGuestRootfs =
        system:
        let
          pkgs = repo.inputs.nixpkgs.legacyPackages.${system};
          guestProbe = self.packages.${system}.guest-probe;
        in
        import ./guest-rootfs.nix { inherit pkgs guestProbe; };

      mkLauncher =
        system:
        let
          pkgs = repo.inputs.nixpkgs.legacyPackages.${system};
          libkrun = repo.packages.${system}.libkrun;
        in
        pkgs.stdenv.mkDerivation {
          pname = "virgl-guest-launcher";
          version = "0.1.0";
          # launcher.c is the build input; run.sh is needed below to wrap it.
          # Restrict the source to just those two files so the whole-repo walk
          # (and its fd exhaustion) is avoided.
          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = [
              ./launcher.c
              ./run.sh
            ];
          };

          nativeBuildInputs = [ pkgs.gcc ];
          buildInputs = [ libkrun ];

          buildPhase = ''
            runHook preBuild
            gcc -O2 -Wall -Wno-deprecated-declarations launcher.c \
              -I${libkrun}/include -L${libkrun}/lib -lkrun \
              -Wl,-rpath,${libkrun}/lib \
              -o launcher-bin
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p "$out/bin"
            cp launcher-bin "$out/bin/launcher"
            cp "$src/run.sh" "$out/bin/run-guest-probe"
            chmod +x "$out/bin/run-guest-probe"
            runHook postInstall
          '';

          meta.mainProgram = "launcher";
        };
    in
    {
      packages = forAllSystems (system: {
        guest-probe = mkGuestProbe system;
        guest-rootfs = mkGuestRootfs system;
        launcher = mkLauncher system;
        default = mkGuestRootfs system;
      });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.launcher}/bin/run-guest-probe";
        };
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = repo.inputs.nixpkgs.legacyPackages.${system};
          libkrun = repo.packages.${system}.libkrun;
        in
        {
          default = pkgs.mkShell {
            buildInputs = [
              pkgs.gcc
              pkgs.vulkan-headers
              libkrun
              pkgs.vulkan-tools
              pkgs.virglrenderer
            ];
            shellHook = ''
              export RENDER_SERVER_EXEC_PATH=${pkgs.virglrenderer}/libexec/virgl_render_server
              export LD_LIBRARY_PATH=${libkrun}/lib:${pkgs.virglrenderer}/lib:"$LD_LIBRARY_PATH"
              echo "virgl guest probe dev shell"
              echo "  RENDER_SERVER_EXEC_PATH=$RENDER_SERVER_EXEC_PATH"
              echo "  Build the rootfs: nix build .#guest-rootfs"
              echo "  Boot + probe:     nix run ."
            '';
          };
        }
      );
    };
}