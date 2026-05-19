{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.18.0";
    srcHash = "sha256-WhNRTBaj0Rl+YOZMN8G99NbZptu25DBNiJMrG6amkgI=";
    npmDepsHash = "sha256-t3yNMKhYi8E25HCq68wsES3tMHaylovdrJXpyKxNHeE=";
    nativeBinarySystems = {
      x86_64-linux = {
        omx-api = {
          asset = "omx-api-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-6yGHU0NfZF9cOPipz/vFCx8QqGULN2quQkXAHA0YNUk=";
        };
        omx-runtime = {
          asset = "omx-runtime-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-8WKWJAZAI5eh/btNbuqmQ0Ooq3F/MI6xs6mLnCpZk4Q=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-Jhsg7bnIawXRC9+HL35eV2XKcGZW8nNqXIgcWfQ46sA=";
        };
        omx-explore-harness = {
          asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-explore-harness";
          hash = "sha256-ANP41G63dQ64RRr1cFetTwSnIHnC6sX0azKxnpAhEkE=";
        };
      };
      aarch64-linux = {
        omx-api = {
          asset = "omx-api-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-pIKGA+Nlgj7pqubE6m5kV2oot7uoTvGw+n8SKjihP3Q=";
        };
        omx-runtime = {
          asset = "omx-runtime-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-Yd9ZupIGEfsAWmsD+tRK39eVE2SoFVu0UO9ZFkcrtVA=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-wnfcLeReW6o0OmIU7vlaWbcdhyJ8Kl6Mp8xfkd10QsQ=";
        };
        omx-explore-harness = {
          asset = "omx-explore-harness-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-explore-harness";
          hash = "sha256-zClftbL+gudhNe+Y9aQqZ/fVBKPU0ZB6gQlHXOncrHg=";
        };
      };
    };
  };

  opencode = {
    version = "1.14.48";
    srcHash = "sha256-gyybqabTco+5ZeWv4lCX8t/R9Jm3tYsA8wVvkrxkEYQ=";
    nodeModulesHash = "sha256-94uXrhyGqW016U6LPE/xIfZGoDOzyUto5DyQrYYePds=";
  };

  piCodingAgent = {
    version = "0.75.3";
    owner = "earendil-works";
    repo = "pi";
    rev = "v0.75.3";
    srcHash = "sha256-c/+cxkp/EZ2PLERxTENN5edXHEs7M2oqzNepjRA4TIE=";
    npmDepsHash = "sha256-4GmM0C7vPDs32TZ6Umso9I3wTZ9IbKjwNEESGkluPFQ=";
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
