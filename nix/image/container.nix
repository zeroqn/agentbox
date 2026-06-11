{ pkgs, pkgsMaster, ohMyCodex, piCodingAgent, rtkPrebuilt, containerLibPolicySeccompJson, libkrun, podman ? pkgs.podman, crun ? pkgs.crun, agentboxMuslPackage, imageVariant }:
let
  configPayloads = import ./config-payloads.nix { inherit pkgs; };
  layers = import ./layers.nix {
    inherit pkgs pkgsMaster ohMyCodex piCodingAgent rtkPrebuilt containerLibPolicySeccompJson libkrun podman crun agentboxMuslPackage;
    fishConfig = configPayloads.fishConfig;
    starshipConfig = configPayloads.starshipConfig;
    inherit imageVariant;
  };
  imageConfig = import ./config.nix {
    inherit pkgs ohMyCodex agentboxMuslPackage configPayloads layers imageVariant;
  };
  imageChecks = import ./checks.nix {
    inherit
      pkgs
      pkgsMaster
      ohMyCodex
      piCodingAgent
      rtkPrebuilt
      containerLibPolicySeccompJson
      libkrun
      podman
      crun
      agentboxMuslPackage
      imageVariant
      ;
  };
  image = pkgs.dockerTools.buildLayeredImage {
    name = "localhost/${imageVariant}";
    tag = "latest";
    maxLayers = layers.agentboxImageMaxLayers;
    contents = layers.imageContents;
    includeNixDB = true;
    layeringPipeline = layers.agentboxImageLayeringPipeline;
    fakeRootCommands = ''
      mkdir -p \
        ./etc \
        ./etc/containers \
        ./home/dev/.cache \
        ./home/dev/.codex \
        ./root \
        ./tmp \
        ./var/empty \
        ./workspace
      chmod 1777 ./tmp
      if [ ! -e ./etc/passwd ]; then
        printf 'root:x:0:0:root:/root:/bin/sh\n' > ./etc/passwd
      else
        sed -i '/^dev:/d' ./etc/passwd
      fi
      if [ ! -e ./etc/group ]; then
        printf 'root:x:0:\n' > ./etc/group
      fi
      printf '%s\n' '${layers.hardenedMallocLib}' > ./etc/ld-nix.so.preload
      chmod 0644 ./etc/ld-nix.so.preload
      cat > ./etc/containers/containers.conf <<'EOF_CONTAINERS_CONF'
      [containers]
      seccomp_profile = "${containerLibPolicySeccompJson}/share/containers/seccomp.json"
      EOF_CONTAINERS_CONF
      chmod 0644 ./etc/containers/containers.conf
      cat > ./etc/tmux.conf <<'EOF_TMUX'
      set-option -g mouse off
      bind-key | split-window -h
      bind-key - split-window -v
      bind-key h select-pane -L
      bind-key l select-pane -R
      bind-key j select-pane -D
      bind-key k select-pane -U
      EOF_TMUX
      chmod 0644 ./etc/tmux.conf
      if ! grep -q '^nixbld:' ./etc/group; then
        printf 'nixbld:x:${toString layers.nixBuilderGroupId}:${layers.nixBuilderGroupMembers}\n' >> ./etc/group
      fi
      cat >> ./etc/passwd <<'EOF_PASSWD'
      ${layers.nixBuilderPasswdEntries}
      EOF_PASSWD
      chown 0:nixbld ./nix/store
      chmod 1775 ./nix/store
      chown -R 1000:1000 ./home/dev ./workspace
    '';

    config = builtins.fromJSON (
      builtins.unsafeDiscardStringContext (builtins.toJSON imageConfig)
    );
  };
in
if imageChecks.missingImageConfigNixDbRefs != [ ] then
  builtins.throw imageChecks.missingRefsMessage
else
  image.overrideAttrs (old: {
    buildCommand = ''
      echo "checking ${imageVariant} image config Nix DB metadata coverage"
      test -e ${imageChecks.imageConfigNixDbRefs}/passed
      echo "checking ${imageVariant} image wrapper contracts"
      test -e ${imageChecks.wrapperContracts}/passed
    '' + (old.buildCommand or "");
  })
