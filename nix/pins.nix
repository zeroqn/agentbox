{
  agentboxVersion = "0.1.0";

  ohMyCodex = {
    version = "0.15.3";
    srcHash = "sha256-4AEk6aS12RNj/duqXV5m7nO2zGdLvzlSypHQfS7apLU=";
    npmDepsHash = "sha256-LUu71MYbUqewhxmc6CiD/vbYOZZfkJITVYqMzEniwYc=";
    exploreHarnessSystems = {
      x86_64-linux = {
        asset = "omx-explore-harness-x86_64-unknown-linux-musl.tar.xz";
        binary = "omx-explore-harness";
        hash = "sha256-jEBei3Nv/aQuEFQkDZz8BcZr3A/jzFm5N4mvJkYAP/4=";
      };
    };
  };

  opencode = {
    version = "1.14.31";
    srcHash = "sha256-VHznPS2OuJ8urQqGK3K0ysQLCk+O8JV7/UCDdFyqafQ=";
    nodeModulesHash = "sha256-f/cWCr6Oqnq21u9+UyhwE5PGqE9X5K+NtjEGbZ4ORPg=";
  };

  piCodingAgent = {
    version = "0.71.0";
    rev = "8040dd6ded6bd52b2b6271ab3588c4474715a4dd";
    srcHash = "sha256-brxktr/vTBLGlhEyfFRrBO0JSy9cJhnZXKk+ucge2uM=";
    npmDepsHash = "sha256-HYzJ0IeE/nltV9DSLJFPBhXvX6x3T3O+WyXh7U4botY=";
  };

  agentboxPrebuiltRelease = {
    owner = "zeroqn";
    repo = "agentbox";
    # Bootstrap value; run scripts/update-agentbox-prebuilt.sh after the
    # first immutable sha-* release is published to pin this to that tag.
    tag = "sha-70c65f004b0e";
    systems = {
      x86_64-linux = {
        asset = "agentbox-x86_64-unknown-linux-musl";
        hash = "sha256-z7PCRPOBvHu/Ircg2tPfu7PjyWDFAuldo6rh2N71FhM=";
      };
    };
  };

  rtkPrebuiltRelease = {
    owner = "rtk-ai";
    repo = "rtk";
    tag = "v0.38.0";
    systems = {
      x86_64-linux = {
        asset = "rtk-x86_64-unknown-linux-musl.tar.gz";
        binary = "rtk";
        hash = "sha256-m6+zVkUPsPZqfy1o0EaNGx4nAWPxYgV05npMj4FtlhA=";
      };
    };
  };
}
