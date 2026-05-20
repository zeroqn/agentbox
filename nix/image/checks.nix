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

  metadataText = builtins.toJSON imageConfig;
  metadataTextFile = pkgs.writeText "agentbox-image-config-metadata.json" (
    builtins.unsafeDiscardStringContext metadataText
  );
  dbClosure = pkgs.closureInfo {
    rootPaths = layers.imageContents;
  };
in
{
  imageConfigNixDbRefs = pkgs.runCommand "agentbox-image-config-nix-db-refs-check"
    {
      nativeBuildInputs = [
        pkgs.coreutils
        pkgs.gnugrep
        pkgs.gnused
      ];
    }
    ''
      set -euo pipefail

      grep -Eo '/nix/store/[0-9a-df-np-sv-z]{32}-[^/":, ]+' ${metadataTextFile} \
        | sort -u > image-config-refs
      sort -u ${dbClosure}/store-paths > nix-db-roots-closure
      comm -23 image-config-refs nix-db-roots-closure > missing-refs

      if [ -s missing-refs ]; then
        echo "agentbox image config references store paths that are not covered by includeNixDB roots." >&2
        echo "These paths can be copied into the image by Docker config/env references but remain absent from Nix validity metadata." >&2
        echo >&2
        echo "Missing from layers.imageContents closure:" >&2
        sed 's/^/  /' missing-refs >&2
        echo >&2
        echo "This check is diagnostic only. It did not repair or mutate Nix DB metadata." >&2
        exit 1
      fi

      mkdir -p "$out"
      cp image-config-refs "$out/image-config-refs"
      cp nix-db-roots-closure "$out/nix-db-roots-closure"
      touch "$out/passed"
    '';
}
