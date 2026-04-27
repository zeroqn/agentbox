{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.15.0";
    srcHash = "sha256-jtyHUtV7N6uKNtvBoqYJU2VYJra6PpcB6hvZhl1ChRE=";
    npmDepsHash = "sha256-LqGRFLAT45mm927PoWnD+q5jroM1/cYod7rG9cFLlqU=";
    exploreHarnessSystems = {
      x86_64-linux = {
        asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
        binary = "omx-explore-harness";
        hash = "sha256-Cu4Rb7ikGxQ/Kwg5JgqTSEhXd5u+sDWqg2rgQJguSjQ=";
      };
    };
  };

  agentboxPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Bootstrap value; run scripts/update-agentbox-prebuilt.sh after the
    # first immutable sha-* release is published to pin this to that tag.
    tag = "sha-545a78653cad";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-2f99/FBQxM26jckGGaiqo/76/bL3H/vMwBPyYTXyY4Q=";
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
