let
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
in
{
  agentboxVersion = cargoToml.workspace.package.version;

  piCodingAgent = {
    version = "0.81.1";
    owner = "earendil-works";
    repo = "pi";
    rev = "v0.81.1";
    srcHash = "sha256-xo3uoR7HceOCL3wqoMcacOe8WXP1o7ReAXne5t6Hgao=";
    npmDepsHash = "sha256-41PY9l89/GBMk4MQVTWFUuf/leklOuHFfcbLkYKy4pI=";
    aiNpmTarballHash = "sha256-x53MD5DU370ZdNoz36P+OWZjGVpoM5sfVcEU2/ckDy8=";
  };

  dirge = {
    version = "0.21.18";
    owner = "dirge-code";
    repo = "dirge";
    rev = "v0.21.18";
    srcHash = "sha256-WLe07SyrBtPShL8qE0u13kPOJBPcJ8jp4CXUzhr35XA=";
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
    tag = "sha-c6bc9b5dfb6b";
    systems = {
      x86_64-linux = {
        asset = "loftd-x86_64-unknown-linux-gnu";
        hash = "sha256-odzh6sgJ1LV1jSdqMJz0TTgACCfLUpQGZf3zcgsEFRQ=";
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
