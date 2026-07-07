{
  agentboxVersion = "0.4.0";

  ohMyCodex = {
    version = "0.18.14";
    srcHash = "sha256-HbPffz5d8KCwGGWJlp+82QUGo98ds3BOhc3vIonPLN0=";
    npmDepsHash = "sha256-oP80bAV9Umc3syfVuYf78+V71KOKQjHsrcGCmqf3xQQ=";
    nativeBinarySystems = {
      x86_64-linux = {
        omx-api = {
          asset = "omx-api-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-b8LbJv4FENxgUGqRk3Yaylt3B7N9DWHFxy/3EG5YGs0=";
        };
        omx-runtime = {
          asset = "omx-runtime-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-jgyH+21aiDBA7NBxTyTTkR386GCCdRfSDzyLi77tLtw=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-FXiS4Ewlnv5nz7Q2YPzGwZVhG1qTWAZ1tNasGWeUz5U=";
        };
      };
      aarch64-linux = {
        omx-api = {
          asset = "omx-api-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-W3nmZkAt76YmPraxA4oHjm88InFUh1FuG4Qq/yz0nNk=";
        };
        omx-runtime = {
          asset = "omx-runtime-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-ibZuop1NS7rHzpZZDkKejX34lSVEZ608R2Bonw1iadQ=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-A1+tHeQ7YzqZZoT/Izy73YH4hDajVTKA15XNASj3N7Q=";
        };
      };
    };
  };

  piCodingAgent = {
    version = "0.79.10";
    owner = "earendil-works";
    repo = "pi";
    rev = "v0.79.10";
    srcHash = "sha256-UMRkOzJpA1XcEHzRwHxgBg6idEmpVJzBKlrXZaVf4MQ=";
    npmDepsHash = "sha256-ZFf9IUI3XPBDOlTqvRkoDvF2EqooMq9Q31YE8iaZkI8=";
  };

  dirge = {
    version = "0.18.13";
    owner = "dirge-code";
    repo = "dirge";
    rev = "v0.18.13";
    srcHash = "sha256-dqHSg+pnT+5dQX/DOQiYDhiAPyLGb355DD5LRk0M+Rg=";
  };

  ompPrebuiltRelease = {
    owner = "can1357";
    repo = "oh-my-pi";
    tag = "v16.2.4";
    systems = {
      x86_64-linux = {
        asset = "omp-linux-x64";
        hash = "sha256-iwDDrVmv156UpuyNKzAFmLJl10LmovjeHQhcD36K1xc=";
      };
      aarch64-linux = {
        asset = "omp-linux-arm64";
        hash = "sha256-jr7Jv/zC4jSOCvW+zqg6WVWhgxWXhoVnpazWpPIFEME=";
      };
    };
  };

  rmuxPrebuiltRelease = {
    owner = "Helvesec";
    repo = "rmux";
    tag = "v0.7.1";
    systems = {
      x86_64-linux = {
        asset = "rmux-0.7.1-linux-x86_64.tar.gz";
        hash = "sha256-shGtBzhT0cF3sXBomCpQwaeGsgmavjuwspFajTKrgVY=";
      };
      aarch64-linux = {
        asset = "rmux-0.7.1-linux-aarch64.tar.gz";
        hash = "sha256-Sc7gOUqoxIiIZ0ETVrogQ3vBiy57TVnO2GoUG6cRFp0=";
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
    tag = "agentbox-7c5b4e1fad84";
    systems = {
      x86_64-linux = {
        asset = "libkrunfw-x86_64-kvm-lto.tgz";
        hash = "sha256-CpVsprAoEghhbU/2ECeFXpOnKX3CHNSFatedALDj/iI=";
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
    tag = "sha-dcd3df561483";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-/scQU8nvWr9nAWHopqwXeNaLBrMK2Lz8rT1oWi0Bpoo=";
      };
    };
  };

  loftdPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Pinned by scripts/update-loftd-prebuilt.sh, which rejects wrapper-script,
    # legacy flake-locked, and concrete /nix/store/<hash>-referencing loftd
    # release payloads.
    tag = "sha-dcd3df561483";
    systems = {
      x86_64-linux = {
        asset = "loftd-x86_64-unknown-linux-gnu";
        hash = "sha256-JaBJcwbBx775LPSU+ftLaS4Nm1froqHt4z5bfrFq5f8=";
      };
    };
  };

  rtkPrebuiltRelease = {
    owner = "rtk-ai";
    repo = "rtk";
    tag = "v0.43.0";
    systems = {
      x86_64-linux = {
        asset = "rtk-x86_64-unknown-linux-musl.tar.gz";
        binary = "rtk";
        hash = "sha256-/4oed2ZJbhdSkaha7KHcl8n/bfM+UeWJPR+8eP6ipgk=";
      };
    };
  };
}
