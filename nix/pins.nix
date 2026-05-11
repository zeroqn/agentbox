{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.16.4";
    srcHash = "sha256-tbQK9Dm7ZqjLgQJfLRcGntzauIz4VwL5VidFQ/yDhdE=";
    npmDepsHash = "sha256-3bQJh8NOiqaFbiM0K4TQwDqwVvitBwjry5/fmbyc3Bw=";
    exploreHarnessSystems = {
      x86_64-linux = {
        asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
        binary = "omx-explore-harness";
        hash = "sha256-QzCrgjJ9ofKMBWfzjskS9qd/XxHEO13wxVet7ajnD9o=";
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

  libkrunfwRelease = {
    owner = "zeroqn";
    repo = "libkrunfw";
    tag = "agentbox-dabd42827ad1";
    systems = {
      x86_64-linux = {
        asset = "libkrunfw-x86_64.tgz";
        hash = "sha256-mV9speOrdgzVfXmwCWhY1SctoYtKfXVR3aNk2tdF9gw=";
      };
      aarch64-linux = {
        asset = "libkrunfw-aarch64.tgz";
        hash = "sha256-tYb2lC1i5jkT7M42FU1pjVW2iNEpJHHx7OSyqpCfzPE=";
      };
      riscv64-linux = {
        asset = "libkrunfw-riscv64.tgz";
        hash = "sha256-9M259kpHs/4tt1sr3NeSpAfxrQY5bMeZfAsmchGFRbc=";
      };
    };
  };

  agentboxPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Bootstrap value; run scripts/update-agentbox-prebuilt.sh after the
    # first immutable sha-* release is published to pin this to that tag.
    tag = "sha-343518be8605";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-FUnEAAfXxOn4B9EM5itI6A/GGFan83HNXIX+cfttdFQ=";
      };
    };
  };

  rtkPrebuiltRelease = {
    owner = "rtk-ai";
    repo = "rtk";
    tag = "v0.39.0";
    systems = {
      x86_64-linux = {
        asset = "rtk-x86_64-unknown-linux-musl.tar.gz";
        binary = "rtk";
        hash = "sha256-BuWCuhmW7wPnakQbmJarp53Rt0bOU50igpbGgbHFQBw=";
      };
    };
  };
}
