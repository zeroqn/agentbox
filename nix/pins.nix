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
    version = "1.14.48";
    srcHash = "sha256-gyybqabTco+5ZeWv4lCX8t/R9Jm3tYsA8wVvkrxkEYQ=";
    nodeModulesHash = "sha256-94uXrhyGqW016U6LPE/xIfZGoDOzyUto5DyQrYYePds=";
  };

  piCodingAgent = {
    version = "0.74.0";
    owner = "earendil-works";
    repo = "pi";
    rev = "v0.74.0";
    srcHash = "sha256-wEiqOezD8w08vyuenh3Kk+YCYBbQoEq67wATDEKy5XM=";
    npmDepsHash = "sha256-oYPxb8RmyprZwmBzzvlke7p3b18Ja5atnpzJW7emK5A=";
  };

  libkrunfwRelease = {
    owner = "zeroqn";
    repo = "libkrunfw";
    tag = "agentbox-07c1ea80fe3c";
    systems = {
      x86_64-linux = {
        asset = "libkrunfw-x86_64-lto.tgz";
        hash = "sha256-XQNUqTqWNxi7WsV9n067JPDGSySP3rZml/Sz506QWvI=";
      };
      aarch64-linux = {
        asset = "libkrunfw-aarch64.tgz";
        hash = "sha256-Yj7/IM3b6IpxjvEbCtx8t1YxlHp+qfH1MEVnrNk4F3E=";
      };
      riscv64-linux = {
        asset = "libkrunfw-riscv64.tgz";
        hash = "sha256-mbE6UZ5ST9iDqGZOqTO6PIka0HxO+amR0KPB1Ugb8b8=";
      };
    };
  };

  agentboxPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Bootstrap value; run scripts/update-agentbox-prebuilt.sh after the
    # first immutable sha-* release is published to pin this to that tag.
    tag = "sha-630176101459";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-r+sqvU0AnNPvSEhdLvf+7zN4zF3Qg/mofIVsxni3OyA=";
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
