{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.18.8";
    srcHash = "sha256-2+vX49GD/IZ1GhWBQC2nqqTV9ipC00W1JdRULVDxw8w=";
    npmDepsHash = "sha256-KZNmwQgzdWUeZZ22NVTWD21JdBgXYRo8eXhYqDkUE3k=";
    nativeBinarySystems = {
      x86_64-linux = {
        omx-api = {
          asset = "omx-api-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-5pfhaS1i14nUYmryvVTD73qDS38LRQ6k+kfHCG86k9w=";
        };
        omx-runtime = {
          asset = "omx-runtime-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-qAhQYh69RavLsM5F2SuMieASFXzJl8CslA7U+O0oXu8=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-SFQ22H3vX5/GQV0HxV+vh+330RHMkuV3zfZk0UeGXHU=";
        };
        omx-explore-harness = {
          asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-explore-harness";
          hash = "sha256-v7MRhLdX2xPrFbz2T6QOFBbVt7fu3ioMPwex5v156n0=";
        };
      };
      aarch64-linux = {
        omx-api = {
          asset = "omx-api-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-6ZmskOuAsAdv/6EWrqAiaLxdVVEyAOl0OxROEQhGzgk=";
        };
        omx-runtime = {
          asset = "omx-runtime-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-jhZucTPlKIkAzUbpaT7INjL3smiqfTQlfgnw6Uy5ayI=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-jBKOmsU5JwxiOHXrcr872+cuHWe43zAEvXNZNi6l/qo=";
        };
        omx-explore-harness = {
          asset = "omx-explore-harness-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-explore-harness";
          hash = "sha256-FCMKhwVqYXCir6DgANn/d7P/86FUJd+ipruu81iNzI8=";
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
    tag = "agentbox-6e3ecf13face";
    systems = {
      x86_64-linux = {
        asset = "libkrunfw-x86_64-kvm-lto.tgz";
        hash = "sha256-hNiT6Ap37K/sXeRScmRgptDaGC5q5uA8LSB19LRgNKk=";
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
    tag = "sha-fd80698f8109";
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
    # Pinned by scripts/update-loftd-prebuilt.sh, which rejects wrapper-script
    # assets and only records raw-ELF loftd release payloads.
    tag = "sha-fd80698f8109";
    systems = {
      x86_64-linux = {
        asset = "loftd-x86_64-linux-flake-locked";
        hash = "sha256-R1psZNFq3EH6EzTTb1nRngGSxZLiaYAnXbgk+hBmuj4=";
      };
    };
  };

  rtkPrebuiltRelease = {
    owner = "rtk-ai";
    repo = "rtk";
    tag = "v0.42.0";
    systems = {
      x86_64-linux = {
        asset = "rtk-x86_64-unknown-linux-musl.tar.gz";
        binary = "rtk";
        hash = "sha256-zdT4esl86Vj3G1OpkYgNatzEHMW8oQRBdaZGMJgBUr4=";
      };
    };
  };
}
