{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.18.1";
    srcHash = "sha256-/9Snmf0GFvc50ISTdB8H+UwQUJYoz+qqpxJOIOmF+Xg=";
    npmDepsHash = "sha256-nK6pcwKmv0fhsu1LBUeKL1NdekhrDa2bUrAkSsl95os=";
    nativeBinarySystems = {
      x86_64-linux = {
        omx-api = {
          asset = "omx-api-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-A1p8KA7cUshxSVcfg+UOanaEPxBk5smGJh+2WnfI8a4=";
        };
        omx-runtime = {
          asset = "omx-runtime-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-A8wid751u7Y/28hKjpxNnlbXEQFMqfblZjbmnc2R7Ys=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-kESM1zjeGrDfmPhyDaThMthdQzKzsW22mUAlUqfjXZk=";
        };
        omx-explore-harness = {
          asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-explore-harness";
          hash = "sha256-Z+dhWzhgn/lZCUTGiL6vsPMvD83K5X3wmror5W4n9zY=";
        };
      };
      aarch64-linux = {
        omx-api = {
          asset = "omx-api-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-b15TgEvf2T/X5QGRmSYQYCJoItA1ZHISwBHqMHj5xNI=";
        };
        omx-runtime = {
          asset = "omx-runtime-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-PUKRJ4DvaasxbImOz3F8PDQMjB78Ci2PIyrEzYE+IJc=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-K0cFYBRgYzfBKFPg765eUSL/utftvUzA+/cQwtzYkZE=";
        };
        omx-explore-harness = {
          asset = "omx-explore-harness-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-explore-harness";
          hash = "sha256-yNmobV00ozlK3TIbY242SWAuya9V1FmSefbMLtJO57E=";
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
    version = "0.75.5";
    owner = "earendil-works";
    repo = "pi";
    rev = "v0.75.5";
    srcHash = "sha256-RNQ4ospdohOA8hyegCMziJHHbmFGdk/QtkjzJmS/PZc=";
    npmDepsHash = "sha256-5eCRPuoeBdybFYPWlmPSJEXl71Nq1cV3CpORq6sfjGs=";
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
    tag = "agentbox-80fea2196c4d";
    systems = {
      x86_64-linux = {
        asset = "libkrunfw-x86_64-kvm-lto.tgz";
        hash = "sha256-CKcc0lzOBhJGB884Db6YrtBnLNCusEd7CLpOR856TgA=";
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
    tag = "sha-3164bfa349c1";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-isGbWdMlRnDCnd4NHg8frNfuRhgYWvQ1FiZC3Mq2d6Y=";
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
