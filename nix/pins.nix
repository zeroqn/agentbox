{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.17.3";
    srcHash = "sha256-AEGXi2pmaxugn9kTVpQ7K8cH+3PaJI5ihxsnNkFEe9E=";
    npmDepsHash = "sha256-QJW+1rucknyaCZCke574FG6yIevH1lcHXJ2DaTdRvQE=";
    exploreHarnessSystems = {
      x86_64-linux = {
        asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
        binary = "omx-explore-harness";
        hash = "sha256-ckp4cee70VMxI/kPMWx/7UWic9AQv3c1H+wG+oY0GAI=";
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

  containerLibPolicySeccompJson = {
    owner = "containers";
    repo = "container-libs";
    rev = "8840603a8795210e1cc80aac1b81eb7acfa9dbee";
    path = "common/pkg/seccomp/seccomp.json";
    hash = "sha256-m3VSAlFq7ktF2dQRq4AMIP5PevlxZqk7fwfVsWwaTs0=";
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
    tag = "sha-c5ed54181a04";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-Zy7/j30t9ROmIEEUXo6Ik7/64BxaZdv82MBoqJ5Xcfk=";
      };
    };
  };

  rtkPrebuiltRelease = {
    owner = "rtk-ai";
    repo = "rtk";
    tag = "v0.40.0";
    systems = {
      x86_64-linux = {
        asset = "rtk-x86_64-unknown-linux-musl.tar.gz";
        binary = "rtk";
        hash = "sha256-p10hCkRYdBBrwW2itO+6AdNtKXr6M+wTRyjy1fQu9a8=";
      };
    };
  };
}
