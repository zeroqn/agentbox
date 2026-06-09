{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.18.10";
    srcHash = "sha256-TmqJe2XqoBkCImviuCONecoWp+Uva1oird/NhHLX/Kc=";
    npmDepsHash = "sha256-5HkxFQhNUORUcct6JGFth5kyFYTgF15qQKRa4wl5cxg=";
    nativeBinarySystems = {
      x86_64-linux = {
        omx-api = {
          asset = "omx-api-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-Qf+OtbVRplKMQyEst0mn4BbAKfuuGI0XIbHjcB6kXw8=";
        };
        omx-runtime = {
          asset = "omx-runtime-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-QwnaR2iapZ5C2P370Csyf5PLJwiLT8YsLRGsgnGlxH8=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-fbvH3H45arYwJhMNm7l/+Ofsrln+hgph1OpuZSIZ/OI=";
        };
        omx-explore-harness = {
          asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-explore-harness";
          hash = "sha256-7nW51FAA1oYH2E+7ZFcRaCpQ84Tq0qbs2hIbjOr6nuw=";
        };
      };
      aarch64-linux = {
        omx-api = {
          asset = "omx-api-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-wvhLMkkSmq6cUkK7KTM/lav0/qycYyj1L+UENgGHuZo=";
        };
        omx-runtime = {
          asset = "omx-runtime-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-HUYSXzUSeazG6v6TkYg2ZR1YHTGO5v6QPmTTNS2xt5Q=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-MFOGxsJO6VK2ZfJL15woF4DtQHqMS+VeWKeFSlB1IbE=";
        };
        omx-explore-harness = {
          asset = "omx-explore-harness-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-explore-harness";
          hash = "sha256-iqJzRtIi2aL7k8W7JwJRSyOr1fc5J9iZExUxnBbwdYM=";
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
    version = "0.76.0";
    owner = "earendil-works";
    repo = "pi";
    rev = "v0.76.0";
    srcHash = "sha256-mlnkSmNJbRfDa0DyGvl0hSV1r2aPszW1G6lz5fAqQeY=";
    npmDepsHash = "sha256-Q1/dE0cZlHX7bAVmfFbym0jpeS6wdZGrDZX8ESSDxgM=";
  };

  reasonix = {
    version = "0.50.0";
    owner = "esengine";
    repo = "DeepSeek-Reasonix";
    rev = "28d95059c72885a2f2a23d5732336488e32374c2";
    srcHash = "sha256-BAUQowb7DKbLHNf9Z4xgZukAKWwVvPOwQ/pSX3gQInM=";
    npmDepsHash = "sha256-ZCAuwe7SBoRcGgqiIpDlv63I88jQZMKkomD1BMVAVbw=";
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
    tag = "agentbox-2c4f2a9c5171";
    systems = {
      x86_64-linux = {
        asset = "libkrunfw-x86_64-kvm-lto.tgz";
        hash = "sha256-oYXzBRuPayONr78eWH5uPU+BZdhRJ+TSuCtTVe+caoE=";
      };
      aarch64-linux = {
        asset = "libkrunfw-aarch64.tgz";
        hash = "sha256-NgwEjhJ2H2ftZXRjqHkaURNW4cre4s4DqKrhkU1uguU=";
      };
      riscv64-linux = {
        asset = "libkrunfw-riscv64.tgz";
        hash = "sha256-lKdq3lmcbO7yG+9eFNmZsBcOTI7cUGXUgyHki6/+1qw=";
      };
    };
  };

  agentboxPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Bootstrap value; run scripts/update-agentbox-prebuilt.sh after the
    # first immutable sha-* release is published to pin this to that tag.
    tag = "sha-35fb1c1f2267";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-U5DvfGK8wJVPCaTyRuJvvCMPjAkKa5VX3ZfqtejImu0=";
      };
    };
  };

  loftdPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Pinned by scripts/update-loftd-prebuilt.sh, which rejects wrapper-script,
    # legacy flake-locked, and concrete /nix/store/<hash>-referencing loftd
    # release payloads.
    tag = "sha-35fb1c1f2267";
    systems = {
      x86_64-linux = {
        asset = "loftd-x86_64-unknown-linux-gnu";
        hash = "sha256-0/Zi+0UBUHmFhlF0ELiK2rMwF2bq0LnFyGc1cjy5fzU=";
      };
    };
  };

  rtkPrebuiltRelease = {
    owner = "rtk-ai";
    repo = "rtk";
    tag = "v0.42.3";
    systems = {
      x86_64-linux = {
        asset = "rtk-x86_64-unknown-linux-musl.tar.gz";
        binary = "rtk";
        hash = "sha256-XfdkpjNwnLhdJIJY0IXSTslfqovKDmg1qTzVfK3E654=";
      };
    };
  };
}
