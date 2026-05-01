{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.15.2";
    srcHash = "sha256-KYRNVv+0L+7v6VvPVo4Vdi9ALcvAH/7WTgxn5Y8cncE=";
    npmDepsHash = "sha256-Zf/nYTFclUQqxEFJVmZY5lie9htVWLov7BdyY9EtnV0=";
    exploreHarnessSystems = {
      x86_64-linux = {
        asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
        binary = "omx-explore-harness";
        hash = "sha256-jrvbvHTx5jz03JQ8Nnm+4jU4wsXhO1YBgDkziDERg8k=";
      };
    };
  };

  opencode = {
    version = "1.14.31";
    srcHash = "sha256-VHznPS2OuJ8urQqGK3K0ysQLCk+O8JV7/UCDdFyqafQ=";
    nodeModulesHash = "sha256-f/cWCr6Oqnq21u9+UyhwE5PGqE9X5K+NtjEGbZ4ORPg=";
  };

  piCodingAgent = {
    version = "0.71.0";
    rev = "8040dd6ded6bd52b2b6271ab3588c4474715a4dd";
    srcHash = "sha256-brxktr/vTBLGlhEyfFRrBO0JSy9cJhnZXKk+ucge2uM=";
    npmDepsHash = "sha256-HYzJ0IeE/nltV9DSLJFPBhXvX6x3T3O+WyXh7U4botY=";
  };

  agentboxPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Bootstrap value; run scripts/update-agentbox-prebuilt.sh after the
    # first immutable sha-* release is published to pin this to that tag.
    tag = "sha-7ccd2d3850f3";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-Ye0zM96tD6LbT+dPGGM863JRV4ByaO8XRRQOY4jnKsI=";
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
