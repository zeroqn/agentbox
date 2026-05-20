{ pkgs, pkgsMaster, ohMyCodex, opencode, piCodingAgent, symposium, rtkPrebuilt, containerLibPolicySeccompJson, libkrun, podman ? pkgs.podman, crun ? pkgs.crun, agentboxMuslPackage }:

let
  configPayloads = import ./config-payloads.nix { inherit pkgs; };
  layers = import ./layers.nix {
    inherit pkgs pkgsMaster ohMyCodex opencode piCodingAgent symposium rtkPrebuilt containerLibPolicySeccompJson libkrun podman crun agentboxMuslPackage;
    fishConfig = configPayloads.fishConfig;
    starshipConfig = configPayloads.starshipConfig;
  };
  imageConfig = import ./config.nix {
    inherit pkgs ohMyCodex agentboxMuslPackage configPayloads layers;
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

  metadataText = builtins.toJSON imageConfig;
  imageMetadataRootsText = builtins.concatStringsSep "\n" (
    map toString layers.imageMetadataNixDbRoots
  );
  imageConfigRefs = storeRefsIn metadataText;
  imageMetadataRoots = storeRefsIn imageMetadataRootsText;
  missingImageConfigNixDbRefs = builtins.filter (
    ref: !(builtins.elem ref imageMetadataRoots)
  ) imageConfigRefs;

  refsText = refs: builtins.concatStringsSep "\n" refs;
  indentedRefsText = refs: builtins.concatStringsSep "\n" (map (ref: "  ${ref}") refs);
  missingRefsMessage = ''
    agentbox image config references store paths outside the static Nix DB root set.
    These paths can be pulled in by Docker config/env references before they are covered by image Nix DB metadata.

    Missing from layers.imageMetadataNixDbRoots:
    ${indentedRefsText missingImageConfigNixDbRefs}

    This check is diagnostic only. It did not repair or mutate Nix DB metadata.
  '';

  imageConfigRefsFile = pkgs.writeText "agentbox-image-config-refs.txt" (
    builtins.unsafeDiscardStringContext (refsText imageConfigRefs)
  );
  imageMetadataRootsFile = pkgs.writeText "agentbox-image-metadata-roots.txt" (
    builtins.unsafeDiscardStringContext (refsText imageMetadataRoots)
  );
  missingRefsFile = pkgs.writeText "agentbox-image-config-missing-refs.txt" (
    builtins.unsafeDiscardStringContext (refsText missingImageConfigNixDbRefs)
  );
  missingRefsMessageFile = pkgs.writeText "agentbox-image-config-missing-refs-message.txt" (
    builtins.unsafeDiscardStringContext missingRefsMessage
  );
in
{
  inherit
    imageConfigRefs
    imageMetadataRoots
    missingImageConfigNixDbRefs
    missingRefsMessage
    ;

  imageConfigNixDbRefs = pkgs.runCommand "agentbox-image-config-nix-db-refs-check"
    {
      nativeBuildInputs = [
        pkgs.coreutils
        pkgs.gnused
      ];
    }
    ''
      set -euo pipefail

      cp ${imageConfigRefsFile} image-config-refs
      cp ${imageMetadataRootsFile} image-metadata-roots
      cp ${missingRefsFile} missing-refs

      if [ -s missing-refs ]; then
        cat ${missingRefsMessageFile} >&2
        exit 1
      fi

      mkdir -p "$out"
      cp image-config-refs "$out/image-config-refs"
      cp image-metadata-roots "$out/image-metadata-roots"
      touch "$out/passed"
    '';
}
