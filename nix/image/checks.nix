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
  imageVariant,
}:

let
  configPayloads = import ./config-payloads.nix { inherit pkgs; };
  layers = import ./layers.nix {
    inherit
      pkgs
      piCodingAgent
      rioBin
      dirge
      ompPrebuilt
      rmuxPrebuilt
      rtkPrebuilt
      containerLibPolicySeccompJson
      libkrun
      wl-cross-domain-proxy
      podman
      crun
      agentboxMuslPackage
      ;
    fishConfig = configPayloads.fishConfig;
    starshipConfig = configPayloads.starshipConfig;
    inherit imageVariant;
  };
  imageConfig = import ./config.nix {
    inherit
      pkgs
      agentboxMuslPackage
      configPayloads
      layers
      imageVariant
      ;
  };

  storePathPattern = "[0-9a-df-np-sv-z]{32}-[^/\":, ]+";
  storeRefDelimiters = [
    "/"
    "\""
    ":"
    ","
    " "
    "\\"
    "\n"
    "\t"
  ];
  takeStoreRefToken =
    segment:
    let
      takeStoreRefChars =
        chars:
        if chars == [ ] then
          [ ]
        else
          let
            head = builtins.head chars;
            tail = builtins.tail chars;
          in
          if builtins.elem head storeRefDelimiters then [ ] else [ head ] ++ takeStoreRefChars tail;
    in
    pkgs.lib.concatStrings (takeStoreRefChars (pkgs.lib.stringToCharacters segment));
  sortStoreRefs = refs: pkgs.lib.unique (pkgs.lib.sort (left: right: left < right) refs);
  storeRefsIn =
    text:
    let
      segments = builtins.tail (pkgs.lib.splitString "/nix/store/" text);
      tokens = map takeStoreRefToken segments;
      refs = map (token: "/nix/store/${token}") (
        builtins.filter (token: builtins.match storePathPattern token != null) tokens
      );
    in
    sortStoreRefs refs;

  imageConfigText = builtins.unsafeDiscardStringContext (builtins.toJSON imageConfig);
  imageConfigRefs = storeRefsIn imageConfigText;
  imageNixDbClosureInfo = pkgs.closureInfo {
    rootPaths = layers.imageContents;
  };
  imageNixDbStorePathsText = builtins.readFile "${imageNixDbClosureInfo}/store-paths";
  imageNixDbStorePaths = storeRefsIn imageNixDbStorePathsText;
  missingImageConfigNixDbRefs = builtins.filter (
    ref: !(builtins.elem ref imageNixDbStorePaths)
  ) imageConfigRefs;

  refsText = refs: builtins.concatStringsSep "\n" refs;
  indentedRefsText = refs: builtins.concatStringsSep "\n" (map (ref: "  ${ref}") refs);
  missingRefsMessage = ''
    ${imageVariant} image config references store paths outside the generated image Nix DB metadata.
    These paths can be pulled in by Docker config/env references without being registered in /nix/var/nix/db.

    Missing from pkgs.closureInfo { rootPaths = layers.imageContents; }:
    ${indentedRefsText missingImageConfigNixDbRefs}

    This check uses the same root path closure that dockerTools.includeNixDB loads into the image DB.
    It does not inspect, repair, or mutate the host Nix DB.
  '';

  imageConfigFile = pkgs.writeText "${imageVariant}-image-config.json" imageConfigText;
  imageConfigRefsFile = pkgs.writeText "${imageVariant}-image-config-refs.txt" (
    refsText imageConfigRefs
  );
  imageNixDbStorePathsFile = pkgs.writeText "${imageVariant}-image-nix-db-store-paths.txt" (
    builtins.unsafeDiscardStringContext imageNixDbStorePathsText
  );
  missingRefsFile = pkgs.writeText "${imageVariant}-image-config-missing-refs.txt" (
    builtins.unsafeDiscardStringContext (refsText missingImageConfigNixDbRefs)
  );
  missingRefsMessageFile = pkgs.writeText "${imageVariant}-image-config-missing-refs-message.txt" (
    builtins.unsafeDiscardStringContext missingRefsMessage
  );
  containerSourceFile = pkgs.writeText "${imageVariant}-container-nix-source.txt" (
    builtins.readFile ./container.nix
  );
  configSourceFile = pkgs.writeText "${imageVariant}-config-nix-source.txt" (
    builtins.readFile ./config.nix
  );
  layersSourceFile = pkgs.writeText "${imageVariant}-layers-nix-source.txt" (
    builtins.readFile ./layers.nix
  );
  allocatorContracts = ''
    grep -F 'mimallocLib = ' ${layersSourceFile}
    grep -F 'pkgs.mimalloc' ${layersSourceFile}
    grep -F './etc/ld-nix.so.preload' ${containerSourceFile}
    grep -F 'cat > ./etc/nix-allocator-libs <<EOF_NIX_ALLOCATOR_LIBS' ${containerSourceFile}
    grep -F 'mimalloc=' ${containerSourceFile}
    grep -F 'hardened=' ${containerSourceFile}
    grep -F 'AGENTBOX_MIMALLOC_LIB=' ${configSourceFile}
    grep -F 'LOFTD_MIMALLOC_LIB=' ${configSourceFile}
    ! grep -F 'LD_PRELOAD=' ${containerSourceFile}
  '';
  terminalMultiplexerContracts = ''
    grep -F 'rmuxPrebuilt' ${layersSourceFile}
    grep -F 'pkgs.tmux' ${layersSourceFile}
    grep -F 'rmuxPrebuilt' ${containerSourceFile}
    grep -F './etc/rmux.conf' ${containerSourceFile}
    grep -F 'bind | split-window -h' ${containerSourceFile}
    grep -F 'bind - split-window -v' ${containerSourceFile}
    grep -F 'bind h select-pane -L' ${containerSourceFile}
    grep -F 'bind j select-pane -D' ${containerSourceFile}
    grep -F 'bind k select-pane -U' ${containerSourceFile}
    grep -F 'bind l select-pane -R' ${containerSourceFile}
    ! grep -F 'rmuxTmuxCommandCompat' ${layersSourceFile}
    ! grep -F 'rmux-tmux-command-compat' ${layersSourceFile}
    ! grep -F './etc/tmux.conf' ${containerSourceFile}
    test -x ${rmuxPrebuilt}/bin/rmux
    test -x ${pkgs.tmux}/bin/tmux
    test "$(grep -Fc 'set -g mouse on' ${containerSourceFile})" -eq 2
    ${rmuxPrebuilt}/bin/rmux -V
    ${pkgs.tmux}/bin/tmux -V
  '';
  ghosttyTerminfoContracts = ''
    ! grep -F 'pkgs.ghostty' ${layersSourceFile}
    grep -F './home/dev/.terminfo/x' ${containerSourceFile}
    grep -F 'pkgs.ghostty.terminfo' ${containerSourceFile}
    grep -F 'xterm-ghostty' ${containerSourceFile}
    test -f ${pkgs.ghostty.terminfo}/share/terminfo/x/xterm-ghostty
    ${pkgs.lib.optionalString (rioBin != null) ''
      test -x ${rioBin}/bin/rio
      test -f ${rioBin}/share/terminfo/r/rio
      test -f ${rioBin}/share/terminfo/x/xterm-rio
    ''}
  '';

  omxAbsent =
    pkgs.runCommand "${imageVariant}-image-omx-absent-check"
      {
        nativeBuildInputs = [ pkgs.gnugrep ];
      }
      ''
        set -euo pipefail

        test ! -e ${layers.agentImageLayer}/bin/omx
        test ! -e ${layers.agentImageLayer}/bin/omx-api
        test ! -e ${layers.agentImageLayer}/bin/omx-runtime
        test ! -e ${layers.agentImageLayer}/bin/omx-sparkshell

        ! grep -F 'OMX_API_BIN=' ${imageConfigFile}
        ! grep -F 'OMX_RUNTIME_BINARY=' ${imageConfigFile}
        ! grep -F 'OMX_SPARKSHELL_BIN=' ${imageConfigFile}
        ! grep -F 'oh-my-codex' ${imageConfigFile}
        ! grep -F 'oh-my-codex' ${imageNixDbStorePathsFile}

        mkdir -p "$out"
        touch "$out/passed"
      '';

  wrapperContracts =
    pkgs.runCommand "${imageVariant}-image-wrapper-contracts-check"
      {
        nativeBuildInputs = [ pkgs.gnugrep ];
      }
      ''
        set -euo pipefail

        ${allocatorContracts}
        ${terminalMultiplexerContracts}
        ${ghosttyTerminfoContracts}

        ${
          if imageVariant == "loftd" then
            ''
              grep -F 'LOFTD_NIX_OVERLAY' ${layers.nixCommandCompat}/bin/nix
              grep -F 'loftd-guest-init internal nix wait' ${layers.nixCommandCompat}/bin/nix
              grep -F 'LOFTD_CONTAINERS_STORAGE' ${layers.podmanCommandCompat}/bin/podman
              grep -F 'loftd-guest-init internal podman wait' ${layers.podmanCommandCompat}/bin/podman
              grep -F 'loftd-guest-init internal podman service-wait' ${layers.dockerCommandCompat}/bin/docker
              grep -F 'loftd-guest-init internal podman service-wait' ${layers.dockerComposeCommandCompat}/bin/docker-compose
              grep -F 'loftd-nix-store-db-check' ${layers.nixStoreDbCheck}/bin/loftd-nix-store-db-check
              grep -F '/run/loftd/nix-disk/upper' ${layers.nixStoreDbCheck}/bin/loftd-nix-store-db-check
              test -x ${pkgs.perf}/bin/perf
              test -x ${pkgs.strace}/bin/strace
              ${pkgs.lib.optionalString (rioBin != null) ''
                test -x ${rioBin}/bin/rio
                grep -F './home/dev/.terminfo/r' ${containerSourceFile}
                grep -F '${"$"}{rioBin}/share/terminfo/r/rio' ${containerSourceFile}
                grep -F '${"$"}{rioBin}/share/terminfo/x/xterm-rio' ${containerSourceFile}
                case ":${layers.imagePath}:" in
                  *":${rioBin}/bin:"*) ;;
                  *) exit 1 ;;
                esac
              ''}
              case ":${layers.imagePath}:" in
                *":${pkgs.perf}/bin:"*) ;;
                *) exit 1 ;;
              esac
              case ":${layers.imagePath}:" in
                *":${pkgs.strace}/bin:"*) ;;
                *) exit 1 ;;
              esac
              ! grep -F 'AGENTBOX_LIBKRUN' ${layers.nixCommandCompat}/bin/nix
              ! grep -F '/run/agentbox/nix-disk/upper' ${layers.nixStoreDbCheck}/bin/loftd-nix-store-db-check
            ''
          else
            ''
              grep -F 'AGENTBOX_LIBKRUN_NIX_OVERLAY' ${layers.nixCommandCompat}/bin/nix
              grep -F 'agentbox-guest-init libkrun nix wait' ${layers.nixCommandCompat}/bin/nix
              grep -F 'AGENTBOX_LIBKRUN_CONTAINERS_STORAGE' ${layers.podmanCommandCompat}/bin/podman
              grep -F 'agentbox-guest-init libkrun podman wait' ${layers.podmanCommandCompat}/bin/podman
              grep -F 'agentbox-guest-init libkrun podman service-wait' ${layers.dockerCommandCompat}/bin/docker
              grep -F 'agentbox-guest-init libkrun podman service-wait' ${layers.dockerComposeCommandCompat}/bin/docker-compose
              grep -F 'agentbox-nix-store-db-check' ${layers.nixStoreDbCheck}/bin/agentbox-nix-store-db-check
              grep -F '/run/agentbox/nix-disk/upper' ${layers.nixStoreDbCheck}/bin/agentbox-nix-store-db-check
              ${pkgs.lib.optionalString (rioBin != null) ''
                case ":${layers.imagePath}:" in
                  *":${rioBin}/bin:"*) exit 1 ;;
                  *) ;;
                esac
              ''}
              case ":${layers.imagePath}:" in
                *":${pkgs.perf}/bin:"*) exit 1 ;;
              esac
              case ":${layers.imagePath}:" in
                *":${pkgs.strace}/bin:"*) exit 1 ;;
              esac
            ''
        }

        mkdir -p "$out"
        touch "$out/passed"
      '';
in
{
  inherit
    imageConfigRefs
    imageNixDbClosureInfo
    imageNixDbStorePaths
    missingImageConfigNixDbRefs
    missingRefsMessage
    omxAbsent
    wrapperContracts
    ;

  imageConfigNixDbRefs =
    pkgs.runCommand "${imageVariant}-image-config-nix-db-refs-check"
      {
        nativeBuildInputs = [
          pkgs.coreutils
        ];
      }
      ''
        set -euo pipefail

        ${allocatorContracts}
        ${terminalMultiplexerContracts}
        ${ghosttyTerminfoContracts}

        cp ${imageConfigRefsFile} image-config-refs
        cp ${imageNixDbStorePathsFile} image-nix-db-valid-paths
        cp ${missingRefsFile} missing-refs

        if [ -s missing-refs ]; then
          cat ${missingRefsMessageFile} >&2
          exit 1
        fi

        mkdir -p "$out"
        cp image-config-refs "$out/image-config-refs"
        cp image-nix-db-valid-paths "$out/image-nix-db-valid-paths"
        touch "$out/passed"
      '';
}
