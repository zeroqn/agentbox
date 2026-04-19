{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.13.2";
    srcHash = "sha256-TdLFlGj+sCwoBXgPLQ8xCc+mBHdSdz5T3kPajEUIK7I=";
    npmDepsHash = "sha256-zBcay5NgEnpnCZd7+KUQFnuPBUo2QZxvPLEMIsgb+F4=";
  };

  agentboxPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Bootstrap value; run scripts/update-agentbox-prebuilt.sh after the
    # first immutable sha-* release is published to pin this to that tag.
    tag = "sha-fe69f232d62a";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-dd4L5gzWWT06VWBONQimTCtjo/QR1NwcuW5C5ENVPcE=";
      };
    };
  };
}
