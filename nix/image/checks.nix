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
  takeStoreRefToken = segment:
    let
      takeStoreRefChars = chars:
        if chars == [ ] then
          [ ]
        else
          let
            head = builtins.head chars;
            tail = builtins.tail chars;
          in
          if builtins.elem head storeRefDelimiters then
            [ ]
          else
            [ head ] ++ takeStoreRefChars tail;
    in
    pkgs.lib.concatStrings (takeStoreRefChars (pkgs.lib.stringToCharacters segment));
  sortStoreRefs = refs: pkgs.lib.unique (pkgs.lib.sort (left: right: left < right) refs);
  storeRefsIn = text:
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

  wrapperContracts = pkgs.runCommand "${imageVariant}-image-wrapper-contracts-check"
    {
      nativeBuildInputs = [ pkgs.gnugrep ];
    }
    ''
      set -euo pipefail

      ${if imageVariant == "loftd" then ''
        grep -F 'LOFTD_NIX_OVERLAY' ${layers.nixCommandCompat}/bin/nix
        grep -F 'loftd-guest-init internal nix wait' ${layers.nixCommandCompat}/bin/nix
        grep -F 'LOFTD_CONTAINERS_STORAGE' ${layers.podmanCommandCompat}/bin/podman
        grep -F 'loftd-guest-init internal podman wait' ${layers.podmanCommandCompat}/bin/podman
        grep -F 'loftd-guest-init internal podman service-wait' ${layers.dockerCommandCompat}/bin/docker
        grep -F 'loftd-guest-init internal podman service-wait' ${layers.dockerComposeCommandCompat}/bin/docker-compose
        grep -F 'loftd-nix-store-db-check' ${layers.nixStoreDbCheck}/bin/loftd-nix-store-db-check
        grep -F '/run/loftd/nix-disk/upper' ${layers.nixStoreDbCheck}/bin/loftd-nix-store-db-check
        ! grep -F 'AGENTBOX_LIBKRUN' ${layers.nixCommandCompat}/bin/nix
        ! grep -F '/run/agentbox/nix-disk/upper' ${layers.nixStoreDbCheck}/bin/loftd-nix-store-db-check
      '' else ''
        grep -F 'AGENTBOX_LIBKRUN_NIX_OVERLAY' ${layers.nixCommandCompat}/bin/nix
        grep -F 'agentbox-guest-init libkrun nix wait' ${layers.nixCommandCompat}/bin/nix
        grep -F 'AGENTBOX_LIBKRUN_CONTAINERS_STORAGE' ${layers.podmanCommandCompat}/bin/podman
        grep -F 'agentbox-guest-init libkrun podman wait' ${layers.podmanCommandCompat}/bin/podman
        grep -F 'agentbox-guest-init libkrun podman service-wait' ${layers.dockerCommandCompat}/bin/docker
        grep -F 'agentbox-guest-init libkrun podman service-wait' ${layers.dockerComposeCommandCompat}/bin/docker-compose
        grep -F 'agentbox-nix-store-db-check' ${layers.nixStoreDbCheck}/bin/agentbox-nix-store-db-check
        grep -F '/run/agentbox/nix-disk/upper' ${layers.nixStoreDbCheck}/bin/agentbox-nix-store-db-check
      ''}

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
    wrapperContracts
    ;

  imageConfigNixDbRefs = pkgs.runCommand "${imageVariant}-image-config-nix-db-refs-check"
    {
      nativeBuildInputs = [
        pkgs.coreutils
      ];
    }
    ''
      set -euo pipefail

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
