use bloomery_daemon::config::load_config;

fn write_temp_toml(name: &str, contents: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("bloomery-daemon-config-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn minimal_toml_fills_defaults() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models]
llama = "/models/llama.gguf"
"#;
    let path = write_temp_toml("minimal.toml", toml);
    let config = load_config(&path).unwrap();

    assert_eq!(config.port, 9000);
    assert_eq!(
        config.data_dir,
        std::path::PathBuf::from("/tmp/bloomery-daemon-test-data")
    );
    assert_eq!(
        config.models.get("llama").unwrap(),
        std::path::Path::new("/models/llama.gguf")
    );
    assert_eq!(config.tier.name, "enthusiast-16gb");
    assert!(!config.tier.emulated);
    assert!(!config.assay.enabled);
    assert_eq!(config.assay.python, "python3");

    // Defaults fill in for everything the minimal TOML didn't specify.
    assert_eq!(config.overhead_mib, 1024);
    assert_eq!(config.default_priority, 100);
    assert_eq!(config.default_budget_tokens, 200_000);
    assert!(!config.allow_unprofiled);
    assert_eq!(config.time_share_quantum_secs, 30);
}

#[test]
fn omitted_port_defaults_to_8181() {
    let toml = r#"
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models]
llama = "/models/llama.gguf"
"#;
    let path = write_temp_toml("no-port.toml", toml);
    let config = load_config(&path).unwrap();
    assert_eq!(config.port, 8181);
}

#[test]
fn omitted_assay_python_defaults_to_python3() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = true }

[models]
llama = "/models/llama.gguf"
"#;
    let path = write_temp_toml("assay-defaults.toml", toml);
    let config = load_config(&path).unwrap();
    assert!(config.assay.enabled);
    assert_eq!(config.assay.python, "python3");
}

/// A `models` table is not defaultable — an empty daemon config with no
/// models is not a legal boot state. The error must name the missing field
/// so an operator staring at a startup failure knows exactly what to add.
#[test]
fn missing_models_table_is_named_error() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }
"#;
    let path = write_temp_toml("missing-models.toml", toml);
    let err = load_config(&path).unwrap_err();
    assert!(
        err.contains("models"),
        "error should name the missing field: {err}"
    );
}
