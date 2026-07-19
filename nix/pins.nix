let
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
in
{
  agentboxVersion = cargoToml.workspace.package.version;

  piCodingAgent = {
    version = "0.80.10";
    owner = "earendil-works";
    repo = "pi";
    rev = "v0.80.10";
    srcHash = "sha256-Vs/ndHYzFyfN4CjPV2zMYblLXe9IuM13UrPJI1VsZEQ=";
    npmDepsHash = "sha256-H8cMgmn4ccxRcYsec6ZjURDTQa1sZ8GvjzweKrPkndg=";
  };

  dirge = {
    version = "0.19.17";
    owner = "dirge-code";
    repo = "dirge";
    rev = "v0.19.17";
    srcHash = "sha256-qoWxrYI6hZIZDEzn83WJRgD5gRz8pQGNNljOKW8rJNE=";
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
    tag = "v0.9.0";
    systems = {
      x86_64-linux = {
        asset = "rmux-0.9.0-linux-x86_64.tar.gz";
        hash = "sha256-5bq7i/cZW4diiwL3ttGntrbLzXXLEoo1Zqxb1Ylv2Sk=";
      };
      aarch64-linux = {
        asset = "rmux-0.9.0-linux-aarch64.tar.gz";
        hash = "sha256-3aaqyXqDdOFv8B6WRN8zDCBdT26pNPd2VAXKN/O8Q48=";
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
    tag = "sha-69fd6a600fd8";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-GUsBOaznqAVkF+Gx/xqGZpgfz5W493KjlcMKJWPzJZ4=";
      };
    };
  };

  loftdPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Pinned by scripts/update-loftd-prebuilt.sh, which rejects wrapper-script,
    # legacy flake-locked, and concrete /nix/store/<hash>-referencing loftd
    # release payloads.
    tag = "sha-69fd6a600fd8";
    systems = {
      x86_64-linux = {
        asset = "loftd-x86_64-unknown-linux-gnu";
        hash = "sha256-HtM6ZfdekArw/LWIOXcA/qON/j4mKnDbDf+HSQp80LY=";
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
