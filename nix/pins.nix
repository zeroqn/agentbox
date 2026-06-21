{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.18.13";
    srcHash = "sha256-1zQBKBspNl2UwXgc3lP8QphXAJD0ooRrPgbyd0JTO8A=";
    npmDepsHash = "sha256-7l/wPoSWxQBVaF6VBdTSrmFHOzUJv1QVa27ptf8/52k=";
    nativeBinarySystems = {
      x86_64-linux = {
        omx-api = {
          asset = "omx-api-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-gSvbm36Rd/TfAyuUEqkhHb2fZEhltUBfXJlnGfj9/i0=";
        };
        omx-runtime = {
          asset = "omx-runtime-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-Ni+5fEm+BJo4hTRtbclvPFAnGAOKzGGTmL4vYxNC4+w=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-i0WekQF4G0CgwTRAJy/RwBxQzRjl5z4ZZxo8QH3XXQo=";
        };
      };
      aarch64-linux = {
        omx-api = {
          asset = "omx-api-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-NwcHPUbWJELjxs/cgYZSlwKQmYlqiD//TMfGTq7O00k=";
        };
        omx-runtime = {
          asset = "omx-runtime-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-3bkyVca9iQgtw88uffFjP/1AOkwtQMQQGRfpSxDOGQQ=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-xTBT1WDGYvvVEWZSkRvoAuPaG3P7FEiQ9eejQ/jgBPs=";
        };
      };
    };
  };

  piCodingAgent = {
    version = "0.79.9";
    owner = "earendil-works";
    repo = "pi";
    rev = "v0.79.9";
    srcHash = "sha256-+h1D51JM4F2iHCzTA57A5/uAzHQBKSlz/7x3/PtQhec=";
    npmDepsHash = "sha256-RokkAJcaLu95r1TJ6VjZeFSUBO114sYCLls4mddRrr4=";
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
    tag = "sha-249d3e20c91d";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-gG/awjLnrnR0gs82rnwVS0oJ+1cE1HLhebRuouIMul0=";
      };
    };
  };

  loftdPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Pinned by scripts/update-loftd-prebuilt.sh, which rejects wrapper-script,
    # legacy flake-locked, and concrete /nix/store/<hash>-referencing loftd
    # release payloads.
    tag = "sha-249d3e20c91d";
    systems = {
      x86_64-linux = {
        asset = "loftd-x86_64-unknown-linux-gnu";
        hash = "sha256-znJ4Lgb3zehpyTvL08MIuxnj5YVIxP6m62ZymSuMVA4=";
      };
    };
  };

  rtkPrebuiltRelease = {
    owner = "rtk-ai";
    repo = "rtk";
    tag = "v0.42.4";
    systems = {
      x86_64-linux = {
        asset = "rtk-x86_64-unknown-linux-musl.tar.gz";
        binary = "rtk";
        hash = "sha256-NJdRFtoR4J5QJQHa91gUPgsi7TpCoQ62f7aTpicNnjY=";
      };
    };
  };
}
