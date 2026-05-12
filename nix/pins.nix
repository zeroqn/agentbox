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
    tag = "agentbox-125c62fe14d8";
    systems = {
      x86_64-linux = {
        asset = "libkrunfw-x86_64.tgz";
        hash = "sha256-tn2QO0cIR80AijBft/3pX/JBQ4EJmg+DclpVegMr4uE=";
      };
      aarch64-linux = {
        asset = "libkrunfw-aarch64.tgz";
        hash = "sha256-9Bf+qQ+/S5Zezn+Yb0xRXtqmdlr5LF9dP3X4gg5rijU=";
      };
      riscv64-linux = {
        asset = "libkrunfw-riscv64.tgz";
        hash = "sha256-8RvevzrW0DTY+bFIqKQOkJ+IVyhU7deHLRE1d8bK+TE=";
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
