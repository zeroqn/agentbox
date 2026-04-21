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
    tag = "sha-31f00d8d0226";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-52inqaXVJ27x5v//iSAYhQw1cjZqc9TU5CvlSE6HWig=";
      };
    };
  };
}
