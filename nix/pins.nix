let
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
in
{
  agentboxVersion = cargoToml.workspace.package.version;

  piCodingAgent = {
    version = "0.85.1";
    owner = "earendil-works";
    repo = "pi";
    rev = "v0.85.1";
    srcHash = "sha256-gU8BSiqqOYt2RRuQONHHGvZeSM5KFQVrwif9bmuUXUc=";
    npmDepsHash = "sha256-6/CE7cCSopNH7cUJDkRLunhhiFDgYkhKi6QRBx8zwes=";
    aiNpmTarballHash = "sha256-r30RmGF5RFzm/oizfVfeIvgjwP/TplyuMcVVt/XpklM=";
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

  # Pinned by scripts/update-herdr.sh (tag + per-system asset hashes).
  herdrPrebuiltRelease = {
    owner = "herdrdev";
    repo = "herdr";
    tag = "v0.9.0";
    systems = {
      x86_64-linux = {
        asset = "herdr-linux-x86_64";
        hash = "sha256-T6GgEVjdgEPaktMbJweAsNzBBgMDjZthysTYGrY/tx8=";
      };
      aarch64-linux = {
        asset = "herdr-linux-aarch64";
        hash = "sha256-nI2yD7fnQnsTjVNnET8WIf/TGfL2XW8AniWUApEV8NI=";
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

  # Pinned by scripts/update-dolt-prebuilt.sh (tag + per-system asset hashes).
  doltPrebuiltRelease = {
    owner = "dolthub";
    repo = "dolt";
    tag = "v2.3.2";
    systems = {
      x86_64-linux = {
        asset = "dolt-linux-amd64.tar.gz";
        hash = "sha256-eilJ+isrN5nuHlfm1kUZqNZdZ1/YMvZGnU4H5aHHKxQ=";
      };
      aarch64-linux = {
        asset = "dolt-linux-arm64.tar.gz";
        hash = "sha256-siMehOBq35XqgcboiUCe56ct5alssDu/G/NDOsdjz5w=";
      };
    };
  };

  # Pinned by scripts/update-beads-prebuilt.sh (tag + per-system asset hashes).
  beadsPrebuiltRelease = {
    owner = "gastownhall";
    repo = "beads";
    tag = "v1.3.0-rc.1";
    systems = {
      x86_64-linux = {
        asset = "beads_1.3.0-rc.1_linux_amd64.tar.gz";
        hash = "sha256-8CO25ild0W82hli6XUv/6ZHDaF3kDm3OknX6WGmr9s4=";
      };
      aarch64-linux = {
        asset = "beads_1.3.0-rc.1_linux_arm64.tar.gz";
        hash = "sha256-NMpPH3ij0nyNgzAu9mydY5bgCfcbGHB6giVTjP31DYw=";
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
    tag = "sha-1de640a65a50";
    systems = {
      x86_64-linux = {
        asset = "loftd-x86_64-unknown-linux-gnu";
        hash = "sha256-5LOki5oE1pIIDiOOod7Gs3axj9zC1VM4HC9UsKIX3iw=";
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

  # Pinned by scripts/update-zvec-grep.sh (srcHash + npmDepsHash).
  zvecGrep = {
    version = "0.2.0";
    owner = "zvec-ai";
    repo = "zvec-grep";
    rev = "v0.2.0";
    srcHash = "sha256-2o/6QWyeZqOy7O8ikO8puqMXmtvWdjS9Y1rNW/SD/Bc=";
    npmDepsHash = "sha256-xEK245edmpn5yG2cT0b8/X6ONs4KmLNxxN1jRt5RZe0=";
  };
}
