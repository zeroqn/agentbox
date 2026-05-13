use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::shell::fish::{materialize_config_files, ShellConfigSources};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[test]
fn shell_config_materialization_installs_fish_starship_payloads() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let source_dir = temp.path().join("source");
    fs::create_dir_all(&source_dir).expect("source dir should be created");
    let fish_source = source_dir.join("agentbox-starship.fish");
    let starship_source = source_dir.join("starship.toml");
    fs::write(&fish_source, "starship init fish | source\n")
        .expect("fish source should be written");
    fs::write(&starship_source, "[hostname]\nssh_only = false\n")
        .expect("starship source should be written");

    let identity = DevIdentity {
        uid: 1000,
        gid: 1000,
        home: temp.path().join("home"),
        shell: PathBuf::from("fish"),
    };
    let sources = ShellConfigSources {
        fish_config: fish_source,
        starship_config: starship_source,
    };

    materialize_config_files(&identity, &sources, false)
        .expect("shell config files should materialize");

    assert_eq!(
        fs::read_to_string(
            identity
                .home
                .join(".config/fish/conf.d/agentbox-starship.fish")
        )
        .expect("fish config should be readable"),
        "starship init fish | source\n"
    );
    assert_eq!(
        fs::read_to_string(identity.home.join(".config/starship.toml"))
            .expect("starship config should be readable"),
        "[hostname]\nssh_only = false\n"
    );
    for dir in [
        ".config/fish/conf.d",
        ".config/fish/completions",
        ".config/fish/functions",
        ".local/share/fish",
        ".cache/starship",
    ] {
        assert!(
            identity.home.join(dir).is_dir(),
            "expected {} to exist",
            identity.home.join(dir).display()
        );
    }
    assert_eq!(
        fs::metadata(identity.home.join(".config/starship.toml"))
            .expect("starship metadata should be readable")
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
}

#[test]
fn shell_config_materialization_preserves_existing_user_config() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let source_dir = temp.path().join("source");
    fs::create_dir_all(&source_dir).expect("source dir should be created");
    let fish_source = source_dir.join("agentbox-starship.fish");
    let starship_source = source_dir.join("starship.toml");
    fs::write(&fish_source, "bundled fish\n").expect("fish source should be written");
    fs::write(&starship_source, "bundled starship\n").expect("starship source should be written");

    let identity = DevIdentity {
        uid: 1000,
        gid: 1000,
        home: temp.path().join("home"),
        shell: PathBuf::from("fish"),
    };
    fs::create_dir_all(identity.home.join(".config/fish/conf.d"))
        .expect("target fish dir should be created");
    fs::write(
        identity.home.join(".config/starship.toml"),
        "custom starship\n",
    )
    .expect("existing starship should be written");
    fs::write(
        identity
            .home
            .join(".config/fish/conf.d/agentbox-starship.fish"),
        "custom fish\n",
    )
    .expect("existing fish config should be written");

    let sources = ShellConfigSources {
        fish_config: fish_source,
        starship_config: starship_source,
    };

    materialize_config_files(&identity, &sources, false)
        .expect("shell config files should materialize");

    assert_eq!(
        fs::read_to_string(identity.home.join(".config/starship.toml"))
            .expect("starship config should be readable"),
        "custom starship\n"
    );
    assert_eq!(
        fs::read_to_string(
            identity
                .home
                .join(".config/fish/conf.d/agentbox-starship.fish")
        )
        .expect("fish config should be readable"),
        "custom fish\n"
    );
}
