{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.14.2";
    srcHash = "sha256-UHVJzRMDxZYoDldl3aFkJNOlMr/gsXlbe1cDpfgdV28=";
    npmDepsHash = "sha256-gGlxQLwp0NBsc/SBUEwJJYPMUKre+txgG8SCIBK7NcA=";
    exploreHarnessSystems = {
      x86_64-linux = {
        asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
        binary = "omx-explore-harness";
        hash = "sha256-wyqN2ZGO+ynV/hIZaPadxwB3qLPb5VJ5TyzalTpY9bI=";
      };
    };
  };

  agentboxPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Bootstrap value; run scripts/update-agentbox-prebuilt.sh after the
    # first immutable sha-* release is published to pin this to that tag.
    tag = "sha-31f00d8d0226";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-52inqaXVJ27x5v//iSAYhQw1cjZqc9TU5CvlSE6HWig=";
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
