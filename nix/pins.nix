{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.14.0";
    srcHash = "sha256-5TwD4q+M2V7VBcaEZXq/fPiAgdksgUfZuyQew+4QJPE=";
    npmDepsHash = "sha256-cL/mooHqQs6BS94PiilNbQGUr4qMLdA3xJyO08jawTA=";
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
