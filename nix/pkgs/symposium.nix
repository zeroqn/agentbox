{ pkgs }:

pkgs.rustPlatform.buildRustPackage rec {
  pname = "symposium";
  version = "0.4.0";

  src = pkgs.fetchCrate {
    inherit pname version;
    hash = "sha256-m2xJbC7CtrCVqG7ZhJO6rB4Th+hxwQWCOcxFEz9Sgzs=";
  };

  cargoHash = "sha256-jWfzYKzGdfzQlPtNUOpllXAtd5UcjogkE14RIJvUXSI=";

  # The crates.io tarball includes integration tests that depend on an
  # unpublished `symposium_testlib` crate from the upstream workspace.
  # Build the released binary and validate it with an install smoke test.
  doCheck = false;
  doInstallCheck = true;

  installCheckPhase = ''
    runHook preInstallCheck

    export XDG_CONFIG_HOME="$TMPDIR/config"
    export XDG_CACHE_HOME="$TMPDIR/cache"
    export XDG_STATE_HOME="$TMPDIR/state"
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME/symposium/logs"

    "$out/bin/cargo-agents" --help >/dev/null

    runHook postInstallCheck
  '';

  meta = {
    description = "AI agent tooling for Rust projects, providing the cargo-agents binary";
    homepage = "https://github.com/symposium-dev/symposium";
    license = with pkgs.lib.licenses; [
      asl20
      mit
    ];
    mainProgram = "cargo-agents";
  };
}
