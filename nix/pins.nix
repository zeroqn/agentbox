{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.18.6";
    srcHash = "sha256-qza+RH+bnGPccjAn1ctqDvqxska6jazEfoQpEs1DsZY=";
    npmDepsHash = "sha256-envgU/u0Mkjc5qrBTZCwcTKCLGPC5eHP2b4hDEpvo1Q=";
    nativeBinarySystems = {
      x86_64-linux = {
        omx-api = {
          asset = "omx-api-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-7t65xgbGOb9SXGXkMJ79JvBX3TrD0FliyzdNIxntaKI=";
        };
        omx-runtime = {
          asset = "omx-runtime-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-5IyU4TxSvbB1QfNJOQl5ibptDoUrJ+7dySU5eIzacSg=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-MzUD19tBnR3eOwHrpnZBh3l2HFu2o/gCDASPEIrxHcQ=";
        };
        omx-explore-harness = {
          asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-explore-harness";
          hash = "sha256-zIaNi2appu4u1ro/udfcsz8s43yKgy8rL7TZBPDHplo=";
        };
      };
      aarch64-linux = {
        omx-api = {
          asset = "omx-api-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-bLYXoDkTj2tY6jR1Mu9jMlxIh7Tt373iJties8qw2sE=";
        };
        omx-runtime = {
          asset = "omx-runtime-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-EdzxLQaBQgM4dl5gXL3vGPIrp5IvkOv9W5ErAaaEjkc=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-7T8H1el4K1tHiTwqGbGUzMJqsnpQAzgQnTKj1IwLqIU=";
        };
        omx-explore-harness = {
          asset = "omx-explore-harness-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-explore-harness";
          hash = "sha256-p4Dia8SsJZHivOhX4IYUC26P38QIXNXaB4Uu6/klG+s=";
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
    tag = "sha-f226a7436e45";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-u3d/rGpBT4Yj3Iywq7TPeJNVkjFnfXlq0FwHO7UqA2s=";
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
