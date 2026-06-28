{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.18.16";
    srcHash = "sha256-r6DdkY4vb/41NAe38bS1YnL9P02/RGf9lOEzzMmtKQY=";
    npmDepsHash = "sha256-b2qJvMi1SwBn6UEtYQEZ95WGjeGu+eSzVxOUHfWzNiU=";
    nativeBinarySystems = {
      x86_64-linux = {
        omx-api = {
          asset = "omx-api-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-cmcxVGVDso/HAjHitmtwUQKeLY1AjeTCOMJ7jsPuPd0=";
        };
        omx-runtime = {
          asset = "omx-runtime-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-4DpeoUhGuV7TwJ/gKJq4VT5cfZK85gxllx6CmWRO4nU=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-x86_64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-y+chnUThnUDImwlRNyY133WvxvkTXgK4EbwNIvovF7c=";
        };
      };
      aarch64-linux = {
        omx-api = {
          asset = "omx-api-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-api";
          hash = "sha256-kwUErDALzSY4RSc5zIfeFl8FtkFs6alDFTZUR9am5IM=";
        };
        omx-runtime = {
          asset = "omx-runtime-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-runtime";
          hash = "sha256-rwmI+WrfvFiYVMuXO2J4xGhpGTUu1SVaSu7by+isXjY=";
        };
        omx-sparkshell = {
          asset = "omx-sparkshell-aarch64-unknown-linux-musl.tar.xz";
          binary = "omx-sparkshell";
          hash = "sha256-+tDFdfCHVVx8zoZUzT8CzxCUGSbrhOq9glFw8O5AeB8=";
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
    npmDepsHash = "sha256-aGCtCSPr69xeaclHS7r+cuWQh1LU3RPpLuDp9Mk4Vcs=";
  };

  ompPrebuiltRelease = {
    owner = "can1357";
    repo = "oh-my-pi";
    tag = "v16.1.14";
    systems = {
      x86_64-linux = {
        asset = "omp-linux-x64";
        hash = "sha256-EcYbyzWeze/X3gfdwRYsFow7ESFtRjfWKqUqn+ynJqY=";
      };
      aarch64-linux = {
        asset = "omp-linux-arm64";
        hash = "sha256-HF1pGM9NYJXS8rEhxrwPjU2RDT5drhWGM2K3d6H/510=";
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
    tag = "sha-4655b4082318";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-li432turqOMWNHEWtYr3RM6SWbt0yi8AiV6qkxX14SU=";
      };
    };
  };

  loftdPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Pinned by scripts/update-loftd-prebuilt.sh, which rejects wrapper-script,
    # legacy flake-locked, and concrete /nix/store/<hash>-referencing loftd
    # release payloads.
    tag = "sha-4655b4082318";
    systems = {
      x86_64-linux = {
        asset = "loftd-x86_64-unknown-linux-gnu";
        hash = "sha256-7RIPfw2EqfepoktzQVt4mfKSwtaYtgjCTEQ/OpNq08U=";
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
