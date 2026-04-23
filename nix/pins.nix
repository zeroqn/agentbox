{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.14.3";
    srcHash = "sha256-2X8GLUF68Mdc50wFEcSBVLw101KTFggEKaYDO/jTF+U=";
    npmDepsHash = "sha256-UHzPLZrOqdIB9BDQ/WJlR1XaxUptkY3Nn7UyJt4wwUc=";
    exploreHarnessSystems = {
      x86_64-linux = {
        asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
        binary = "omx-explore-harness";
        hash = "sha256-Q+aMvO9XLdkdRrd3gx1VrNogxbGKjqbQffbcj53DbEQ=";
      };
    };
  };

  agentboxPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Bootstrap value; run scripts/update-agentbox-prebuilt.sh after the
    # first immutable sha-* release is published to pin this to that tag.
    tag = "sha-b380a726272f";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-OY4dM1tq7CY+C5hhgwZO6mDjKiMDHzqeBWQ9e0OsESE=";
      };
    };
  };

  rtkPrebuiltRelease = {
    owner = "rtk-ai";
    repo = "rtk";
    tag = "v0.37.2";
    systems = {
      x86_64-linux = {
        asset = "rtk-x86_64-unknown-linux-musl.tar.gz";
        binary = "rtk";
        hash = "sha256-Pft6BWNqaGh7ocWqaW+o1fy0lER97YbZ64uItxAKN8Y=";
      };
    };
  };
}
