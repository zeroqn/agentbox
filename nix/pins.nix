{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.18.3";
    srcHash = "sha256-oGZc9zHgb1wYoHXY5d+br0SSwt2m+MPI2cmABS6t4BQ=";
    npmDepsHash = "sha256-dD/g8f6ngGQruIm3R0KCom/YZqCEatAvuwQdf7ndPRk=";
    nativeBinarySystems = {
      x86_64-linux = {
        omx-api = {
          asset = "omx-api-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-Tq14s8d3bJdSWXuDKsx50F/ncnMaU2YdYRPwkbgbamU=";
        };
        omx-runtime = {
          asset = "omx-runtime-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-0jHevHLzqIInyUTC/4EYfyO+3S54b8pZTS0mL81hT9o=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-+QzNKYySfKlhQztAE1XjNwG5UtdelUclmLRmkpqC/U0=";
        };
        omx-explore-harness = {
          asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-explore-harness";
          hash = "sha256-h4+v6OEoSj6vdmijQM3sbVPlqJxS+MXba9m7/mWKM0E=";
        };
      };
      aarch64-linux = {
        omx-api = {
          asset = "omx-api-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-n6ZT+IZg/V1Rf3gisU1rpsQOSy6r+VpkEe8/7axyZQA=";
        };
        omx-runtime = {
          asset = "omx-runtime-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-t3pEZCDnHHwYqcAJ76aOSAkJFLhULK+jow5ZI1WAnsY=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-xufCCttm25CJRYdS3DZ4QfNJgatkdSW22msggVBzdaU=";
        };
        omx-explore-harness = {
          asset = "omx-explore-harness-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-explore-harness";
          hash = "sha256-uoGYZ45rABaagqYMEjeO0w8rIzgFVG1Q2omQZzunpq0=";
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
    tag = "sha-eee1150b6b12";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-mJovfs+seEpSrkoqfyN7LET/CO2b2Op7e+PznSXLpdQ=";
      };
    };
  };

  rtkPrebuiltRelease = {
    owner = "rtk-ai";
    repo = "rtk";
    tag = "v0.41.0";
    systems = {
      x86_64-linux = {
        asset = "rtk-x86_64-unknown-linux-musl.tar.gz";
        binary = "rtk";
        hash = "sha256-kK4Q9cdt6brK7F7u77YBL3TdR/TigOxhQpVVW2Taa1c=";
      };
    };
  };
}
