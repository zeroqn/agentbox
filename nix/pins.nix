{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.14.4";
    srcHash = "sha256-S862m/KniIClGxwjGi1/3dCDqsaySwTll6uSJ5SFIac=";
    npmDepsHash = "sha256-25FD3k89kwz9gq9+9h8BGiCEj5Rl/JgnKvrGaxnKENQ=";
    exploreHarnessSystems = {
      x86_64-linux = {
        asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
        binary = "omx-explore-harness";
        hash = "sha256-8JbgDyIIPa1pLJm2PQcjb0P1s1LBGSHwtqRT4dMlNek=";
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
