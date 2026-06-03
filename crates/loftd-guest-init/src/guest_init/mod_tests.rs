#[test]
fn guest_init_uses_modular_runtime_dispatcher() {
    let files = [
        "command",
        "components",
        "fs",
        "process",
        "profile",
        "runtime",
    ];
    for file in files {
        assert!(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/guest_init")
                .join(match file {
                    "components" | "runtime" => format!("{file}/mod.rs"),
                    _ => format!("{file}.rs"),
                })
                .exists(),
            "missing modular guest_init/{file}"
        );
    }
}
