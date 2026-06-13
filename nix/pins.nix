{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.18.11";
    srcHash = "sha256-buv/faGSGhpH2YQnBFcvPOHwTMjssbXTOnkxEQzuhuI=";
    npmDepsHash = "sha256-2+yHyAU78i0i0xzwhncxdHts8d7bKeGJblS4+XISGsw=";
    nativeBinarySystems = {
      x86_64-linux = {
        omx-api = {
          asset = "omx-api-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-GroJD9Gnf0nN81ulFjAm/f9rezqbyEmpC8bmHiUZHC0=";
        };
        omx-runtime = {
          asset = "omx-runtime-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-c2BrXKQvIUjFQzxM5CIQ8Nrk+l5aR5pXkI3OigyO4wM=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-h2glysQuzlyqYaPI4JXUUmMiOz//Z5KZt/SyrDRA07I=";
        };
      };
      aarch64-linux = {
        omx-api = {
          asset = "omx-api-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-8GatW6mG8uz5VtisgbuMMaDX9Eo26JDGVEQdxk8ySqo=";
        };
        omx-runtime = {
          asset = "omx-runtime-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-zvnblLA7egQe25yFbnEinw9Zes07lKdLOJ26ha0jT4M=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-WT92Kpfh2ikLTXeWbHKSCPtzYjLod5B1FYCO0s/6OXA=";
        };
      };
    };
  };

  piCodingAgent = {
    version = "0.79.2";
    owner = "earendil-works";
    repo = "pi";
    rev = "v0.79.2";
    srcHash = "sha256-LsmGGURENKM1r56dfCZesyyL+4hftmgGczMNYGmOlv0=";
    npmDepsHash = "sha256-0QY//6NlbZWhDgsYuEOv+PY5RctT7gwChZ9Ld1MvUIs=";
  };

  ompPrebuiltRelease = {
    owner = "can1357";
    repo = "oh-my-pi";
    tag = "v15.12.3";
    systems = {
      x86_64-linux = {
        asset = "omp-linux-x64";
        hash = "sha256-UJS7H7fkBtiRNuSUSN6Yrp6cBf6xBmo5si00BgLTF90=";
      };
      aarch64-linux = {
        asset = "omp-linux-arm64";
        hash = "sha256-BjXWjM1P7L4rF1E80wD7gv90qJ+c26WgM8iR9VMFus8=";
      };
    };
  };

  containerLibPolicySeccompJson = {
    owner = "containers";
    repo = "container-libs";
    rev = "8840603a8795210e1cc80aac1b81eb7acfa9dbee";
    path = "common/pkg/seccomp/seccomp.json";
    hash = "sha256-m3VSAlFq7ktF2dQRq4AMIP5PevlxZqk7fwfVsWwaTs0=";
  };

  libkrunRelease = {
    owner = "zeroqn";
    repo = "libkrun";
    tag = "loftd-ad8a40428d15";
    systems = {
      x86_64-linux = {
        asset = "libkrun-x86_64-linux-full.tgz";
        hash = "sha256-VzACYEPtXk6+OH1EDQj3fmBkonlUubPsfuXxolxVbWw=";
      };
      aarch64-linux = {
        asset = "libkrun-aarch64-linux-full.tgz";
        hash = "sha256-EXkJT3r+s6TDigY0h8djTxxx8bqtsT2wucxZUZzfGQs=";
      };
    };
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
    tag = "sha-bc8d30550c3c";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-omYNzHWwfbP994gccgsnDypsttKliyWFnkEp9qWA9Ig=";
      };
    };
  };

  loftdPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Pinned by scripts/update-loftd-prebuilt.sh, which rejects wrapper-script,
    # legacy flake-locked, and concrete /nix/store/<hash>-referencing loftd
    # release payloads.
    tag = "sha-7daedc1b9ea2";
    systems = {
      x86_64-linux = {
        asset = "loftd-x86_64-unknown-linux-gnu";
        hash = "sha256-samgw4kPlSfkP5LoydN6J4WV1KqjylOevsW88+MJgJo=";
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
