let
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
in
{
  agentboxVersion = cargoToml.workspace.package.version;

  piCodingAgent = {
    version = "0.84.2";
    owner = "earendil-works";
    repo = "pi";
    rev = "v0.84.2";
    srcHash = "sha256-d29ft9otYxdHRWYIAX8KMHPpppToX9ME5LbPb1rPcYo=";
    npmDepsHash = "sha256-cx1796NfsTUPWiKUsrvDbMryTWYLEF4svZZMUb31pzI=";
    aiNpmTarballHash = "sha256-AmJ4Wnaw6y7sWWzYp6su4j7vidLvG7EhHE8KGUTaz0E=";
  };

  dirge = {
    version = "0.24.0";
    owner = "dirge-code";
    repo = "dirge";
    rev = "v0.24.0";
    srcHash = "sha256-bBVvelpQ3Iv0VayBgWk6Fz8azasTg6mylXQSouB26lk=";
  };

  dirgeSandboxPrebuiltRelease = {
    owner = "zeroqn";
    repo = "dirge";
    tag = "ds-sandbox";
    systems = {
      x86_64-linux = {
        asset = "dirge-x86_64-unknown-linux-gnu-sandbox.tar.gz";
        hash = "sha256-Bo72wotCZ4g1te3/VTQ1w1gYBpd9cmxwhfV699r3/6w=";
      };
    };
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
    tag = "v0.10.0";
    systems = {
      x86_64-linux = {
        asset = "rmux-0.10.0-linux-x86_64.tar.gz";
        hash = "sha256-G+wR7/CMMxPDpAAZbnqT0AuK1KJPge8T3rsDNVwmlsU=";
      };
      aarch64-linux = {
        asset = "rmux-0.10.0-linux-aarch64.tar.gz";
        hash = "sha256-fpFlYOoPuQhkuMJOXQ+BtOPgsBO4qtWrU4Odfo5eGSY=";
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
    tag = "loftd-3842e7383799";
    systems = {
      x86_64-linux = {
        asset = "libkrun-x86_64-linux-full.tgz";
        hash = "sha256-BpmztMASl/H8jqtYWR3f5I9tOsup3odc2SvOxHDhjuY=";
      };
      aarch64-linux = {
        asset = "libkrun-aarch64-linux-full.tgz";
        hash = "sha256-eAmWOjaZiW/brzk+e+OP07OFk17Fv/oVeLG5l8u7Aws=";
      };
    };
  };

  libkrunfwRelease = {
    owner = "zeroqn";
    repo = "libkrunfw";
    tag = "agentbox-e7e571ef6b03";
    systems = {
      x86_64-linux = {
        asset = "libkrunfw-x86_64-kvm-lto.tgz";
        hash = "sha256-Mrx6I4qcTlUlE2FFDx5XvVFcvojnXTtYxJtDEukkWHs=";
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
    tag = "sha-3cf19afed03c";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-YFgSbZxI1xpTCi1W+ATHaFKGSXtiIDU6EHJhCLKBfXQ=";
      };
    };
  };

  loftdPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Pinned by scripts/update-loftd-prebuilt.sh, which rejects wrapper-script,
    # legacy flake-locked, and concrete /nix/store/<hash>-referencing loftd
    # release payloads.
    tag = "sha-22457387b15f";
    systems = {
      x86_64-linux = {
        asset = "loftd-x86_64-unknown-linux-gnu";
        hash = "sha256-IJjO7L/Tzcr5doKAefUN88KkM9sfb4Nz5TH/PoqP8T4=";
      };
    };
  };

  rtkPrebuiltRelease = {
    owner = "rtk-ai";
    repo = "rtk";
    tag = "v0.45.0";
    systems = {
      x86_64-linux = {
        asset = "rtk-x86_64-unknown-linux-musl.tar.gz";
        binary = "rtk";
        hash = "sha256-xMA2+/GB/FXvMpeGyMF+DUJ5crBTuCWUTZaKaq/vG6Q=";
      };
    };
  };
}
