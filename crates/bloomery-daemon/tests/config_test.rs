use bloomery_daemon::config::{load_config, EnvelopeLens};

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
        config.models.get("llama").unwrap().path(),
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

    // Task 5's task surface is dark by default, and its exec bounds default
    // to the numbers named in the Task 5 brief.
    assert!(!config.tasks_enabled);
    assert_eq!(config.read_cap_bytes, 262_144);
    assert_eq!(config.find_result_cap, 100);
    assert_eq!(config.run_output_cap_bytes, 65_536);
    assert_eq!(config.run_timeout_secs, 120);

    // A config that omits `assay.probe_timeout_secs` keeps today's 600s
    // behavior byte-for-byte.
    assert_eq!(config.assay.probe_timeout_secs, 600);
}

/// An operator raising the timeout for a slow, partially-offloaded model
/// (the measured motivation: a qwen3.8-27b Q3 at ~15.5 tok/s projects a
/// `--quick` probe at ~25-30 min) must have that value parse and stick.
#[test]
fn explicit_probe_timeout_secs_parses() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = true, python = "python3", probe_timeout_secs = 1800 }

[models]
llama = "/models/llama.gguf"
"#;
    let path = write_temp_toml("probe-timeout.toml", toml);
    let config = load_config(&path).unwrap();
    assert_eq!(config.assay.probe_timeout_secs, 1800);
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

/// A bare string entry in the models table parses as a ModelSpec::Path
/// variant, with all accessors returning the path and None for the tuning
/// fields.
#[test]
fn bare_string_model_entry_parses() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models]
"qwen3:14b" = "/mnt/extra/models/qwen3-14b.gguf"
"#;
    let path = write_temp_toml("bare-string-model.toml", toml);
    let config = load_config(&path).unwrap();

    assert_eq!(config.models.len(), 1);
    let model = config.models.get("qwen3:14b").unwrap();
    assert_eq!(
        model.path(),
        std::path::Path::new("/mnt/extra/models/qwen3-14b.gguf")
    );
    assert_eq!(model.n_gpu_layers(), None);
    assert_eq!(model.weights_vram_mib(), None);
    assert!(
        !model.think_preseed(),
        "a bare path entry must default think_preseed to false"
    );
}

/// A table entry with path, n_gpu_layers, and weights_vram_mib parses and
/// all accessors return the configured values.
#[test]
fn tuned_model_entry_with_all_fields_parses() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models."qwen3.8:27b"]
path = "/mnt/extra/models/qwen3.8-27b.gguf"
n_gpu_layers = 28
weights_vram_mib = 11264
"#;
    let path = write_temp_toml("tuned-model-full.toml", toml);
    let config = load_config(&path).unwrap();

    assert_eq!(config.models.len(), 1);
    let model = config.models.get("qwen3.8:27b").unwrap();
    assert_eq!(
        model.path(),
        std::path::Path::new("/mnt/extra/models/qwen3.8-27b.gguf")
    );
    assert_eq!(model.n_gpu_layers(), Some(28));
    assert_eq!(model.weights_vram_mib(), Some(11264));
}

/// A table entry with only a path (omitting optional tuning fields) parses,
/// and the tuning accessors return None.
#[test]
fn tuned_model_entry_with_only_path_parses() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models."qwen3:14b"]
path = "/mnt/extra/models/qwen3-14b.gguf"
"#;
    let path = write_temp_toml("tuned-model-minimal.toml", toml);
    let config = load_config(&path).unwrap();

    assert_eq!(config.models.len(), 1);
    let model = config.models.get("qwen3:14b").unwrap();
    assert_eq!(
        model.path(),
        std::path::Path::new("/mnt/extra/models/qwen3-14b.gguf")
    );
    assert_eq!(model.n_gpu_layers(), None);
    assert_eq!(model.weights_vram_mib(), None);
    assert!(
        !model.think_preseed(),
        "a table entry that omits think_preseed must default to false"
    );
}

/// A table entry with `think_preseed = true` parses, and the accessor
/// reflects it (protocol §10, Amendment 2 — the envelope-v2 lens is an
/// explicit per-model operator choice).
#[test]
fn think_preseed_true_parses() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models."qwen3.8:27b"]
path = "/mnt/extra/models/qwen3.8-27b.gguf"
think_preseed = true
"#;
    let path = write_temp_toml("think-preseed-true.toml", toml);
    let config = load_config(&path).unwrap();

    let model = config.models.get("qwen3.8:27b").unwrap();
    assert!(model.think_preseed());
}

/// A config mixing both bare-string and table-entry model shapes parses
/// correctly.
#[test]
fn mixed_model_entries_parse() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models]
"qwen3:14b" = "/mnt/extra/models/qwen3-14b.gguf"

[models."qwen3.8:27b"]
path = "/mnt/extra/models/qwen3.8-27b.gguf"
n_gpu_layers = 28
weights_vram_mib = 11264
"#;
    let path = write_temp_toml("mixed-models.toml", toml);
    let config = load_config(&path).unwrap();

    assert_eq!(config.models.len(), 2);

    let model1 = config.models.get("qwen3:14b").unwrap();
    assert_eq!(
        model1.path(),
        std::path::Path::new("/mnt/extra/models/qwen3-14b.gguf")
    );
    assert_eq!(model1.n_gpu_layers(), None);
    assert_eq!(model1.weights_vram_mib(), None);

    let model2 = config.models.get("qwen3.8:27b").unwrap();
    assert_eq!(
        model2.path(),
        std::path::Path::new("/mnt/extra/models/qwen3.8-27b.gguf")
    );
    assert_eq!(model2.n_gpu_layers(), Some(28));
    assert_eq!(model2.weights_vram_mib(), Some(11264));
}

/// The design spec §2 example block parses character-for-character, verbatim from
/// docs/superpowers/specs/2026-08-15-partial-offload-capability-window-design.md
/// lines 31-41, including comments, ellipsis characters, and exact path spellings.
#[test]
fn spec_section_2_example_parses() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models]
# today's shape — unchanged, still valid
"qwen3:14b" = "/mnt/extra/ollama-models/blobs/sha256-…"

# new shape — per-model tuning
[models."qwen3.8:27b"]
path = "/mnt/extra/ollama-models/blobs/sha256-f5f1dd89…"
n_gpu_layers = 28          # optional; omitted = full offload
weights_vram_mib = 11264   # optional; omitted = charge full weights
"#;
    let path = write_temp_toml("spec-example.toml", toml);
    let config = load_config(&path).unwrap();

    assert_eq!(config.models.len(), 2);

    let model1 = config.models.get("qwen3:14b").unwrap();
    assert_eq!(
        model1.path(),
        std::path::Path::new("/mnt/extra/ollama-models/blobs/sha256-…")
    );
    assert_eq!(model1.n_gpu_layers(), None);
    assert_eq!(model1.weights_vram_mib(), None);

    let model2 = config.models.get("qwen3.8:27b").unwrap();
    assert_eq!(
        model2.path(),
        std::path::Path::new("/mnt/extra/ollama-models/blobs/sha256-f5f1dd89…")
    );
    assert_eq!(model2.n_gpu_layers(), Some(28));
    assert_eq!(model2.weights_vram_mib(), Some(11264));
}

/// A table entry missing the required `path` field fails to parse (serde
/// untagged requires a distinguishing field per variant).
#[test]
fn table_entry_without_path_fails() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models."invalid-model"]
n_gpu_layers = 5
"#;
    let path = write_temp_toml("missing-path.toml", toml);
    let err = load_config(&path).unwrap_err();
    assert!(
        err.contains("path") || err.contains("data"),
        "error should indicate missing path field: {err}"
    );
}

// ---------------------------------------------------------------------------
// Protocol §11 (Amendment 3): the `envelope` enum + `kv_per_token_bytes`.
// ---------------------------------------------------------------------------

fn envelope_toml(envelope: &str) -> String {
    format!(
        r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = {{ name = "enthusiast-16gb", emulated = false }}
assay = {{ enabled = false, python = "python3" }}

[models."qwen3.8:27b"]
path = "/mnt/extra/models/qwen3.8-27b.gguf"
envelope = "{envelope}"
"#
    )
}

/// All three envelope values parse and resolve to the matching
/// [`EnvelopeLens`] variant.
#[test]
fn envelope_parses_for_all_three_values() {
    for (raw, expected) in [
        ("v1", EnvelopeLens::V1),
        ("v2", EnvelopeLens::V2),
        ("v3", EnvelopeLens::V3),
    ] {
        let path = write_temp_toml(&format!("envelope-{raw}.toml"), &envelope_toml(raw));
        let config = load_config(&path).unwrap_or_else(|e| panic!("envelope {raw:?}: {e}"));
        let model = config.models.get("qwen3.8:27b").unwrap();
        assert_eq!(
            model.envelope_lens().unwrap(),
            expected,
            "envelope = {raw:?} must resolve to {expected:?}"
        );
    }
}

/// An unrecognized `envelope` value is a named config error that lists the
/// valid set — never a silent fallback to a default. Caught at `load_config`
/// time (protocol §11: "validated at load"), not deferred to first use.
#[test]
fn unknown_envelope_value_is_a_named_error_mentioning_the_valid_set() {
    let path = write_temp_toml("envelope-bogus.toml", &envelope_toml("v99"));
    let err = load_config(&path).unwrap_err();
    assert!(
        err.contains("v99"),
        "error should name the bad value: {err}"
    );
    assert!(err.contains("v1"), "error should list the valid set: {err}");
    assert!(err.contains("v2"), "error should list the valid set: {err}");
    assert!(err.contains("v3"), "error should list the valid set: {err}");
}

/// The shipped `think_preseed = true` key is accepted as an alias for
/// `envelope = "v2"` when `envelope` itself is absent (protocol §11).
#[test]
fn think_preseed_true_alone_aliases_to_v2() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models."qwen3.8:27b"]
path = "/mnt/extra/models/qwen3.8-27b.gguf"
think_preseed = true
"#;
    let path = write_temp_toml("envelope-alias-v2.toml", toml);
    let config = load_config(&path).unwrap();
    let model = config.models.get("qwen3.8:27b").unwrap();
    assert_eq!(model.envelope_lens().unwrap(), EnvelopeLens::V2);
}

/// Absent `envelope` and absent/`false` `think_preseed` both resolve to
/// `V1` — byte-for-byte today's behavior for every existing config.
#[test]
fn absent_envelope_and_think_preseed_resolves_to_v1() {
    let path = write_temp_toml(
        "envelope-absent.toml",
        r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models."qwen3.8:27b"]
path = "/mnt/extra/models/qwen3.8-27b.gguf"
"#,
    );
    let config = load_config(&path).unwrap();
    let model = config.models.get("qwen3.8:27b").unwrap();
    assert_eq!(model.envelope_lens().unwrap(), EnvelopeLens::V1);

    // The bare-path shape resolves to V1 too — no tuning table at all.
    let bare_path = write_temp_toml(
        "envelope-bare-path.toml",
        r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models]
"qwen3:14b" = "/mnt/extra/models/qwen3-14b.gguf"
"#,
    );
    let config = load_config(&bare_path).unwrap();
    let model = config.models.get("qwen3:14b").unwrap();
    assert_eq!(model.envelope_lens().unwrap(), EnvelopeLens::V1);
}

/// `envelope`/`think_preseed` conflict combos are named config errors, never
/// a silent pick — the three disagreeing pairs protocol §11 names verbatim.
#[test]
fn conflicting_envelope_and_think_preseed_combos_are_named_errors() {
    let cases = [
        ("v1", true),  // think_preseed=true with envelope="v1"
        ("v2", false), // think_preseed=false with envelope="v2"
        ("v3", false), // think_preseed=false with envelope="v3"
    ];
    for (envelope, think_preseed) in cases {
        let toml = format!(
            r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = {{ name = "enthusiast-16gb", emulated = false }}
assay = {{ enabled = false, python = "python3" }}

[models."qwen3.8:27b"]
path = "/mnt/extra/models/qwen3.8-27b.gguf"
envelope = "{envelope}"
think_preseed = {think_preseed}
"#
        );
        let path = write_temp_toml(
            &format!("envelope-conflict-{envelope}-{think_preseed}.toml"),
            &toml,
        );
        let err = load_config(&path).unwrap_err();
        assert!(
            err.contains("envelope") && err.contains("think_preseed"),
            "envelope={envelope:?} think_preseed={think_preseed}: error must name both \
             conflicting keys: {err}"
        );
    }
}

/// Consistent (non-conflicting) pairs are never errors: `envelope = "v3"`
/// with `think_preseed = true` is fine (v3 implies the pre-seed v2 already
/// requires), and `envelope = "v1"` with `think_preseed` absent/`false` is
/// fine too.
#[test]
fn consistent_envelope_and_think_preseed_combos_parse() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models."qwen3.8:27b"]
path = "/mnt/extra/models/qwen3.8-27b.gguf"
envelope = "v3"
think_preseed = true
"#;
    let path = write_temp_toml("envelope-consistent-v3-true.toml", toml);
    let config = load_config(&path).unwrap();
    assert_eq!(
        config
            .models
            .get("qwen3.8:27b")
            .unwrap()
            .envelope_lens()
            .unwrap(),
        EnvelopeLens::V3
    );
}

/// `EnvelopeLens::lens_name` returns the exact pinned strings protocol
/// §10/§11 name.
#[test]
fn envelope_lens_names_are_pinned() {
    assert_eq!(EnvelopeLens::V1.lens_name(), "bloomery-task-envelope-v1");
    assert_eq!(EnvelopeLens::V2.lens_name(), "bloomery-task-envelope-v2");
    assert_eq!(EnvelopeLens::V3.lens_name(), "bloomery-task-envelope-v3");
}

/// `kv_per_token_bytes` parses when present, and is `None` when absent
/// (spec §10 addendum).
#[test]
fn kv_per_token_bytes_parses_and_defaults_to_none() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models."qwen3.8:27b"]
path = "/mnt/extra/models/qwen3.8-27b.gguf"
kv_per_token_bytes = 67109
"#;
    let path = write_temp_toml("kv-per-token-bytes.toml", toml);
    let config = load_config(&path).unwrap();
    let model = config.models.get("qwen3.8:27b").unwrap();
    assert_eq!(model.kv_per_token_bytes(), Some(67109));

    let absent_toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models."qwen3.8:27b"]
path = "/mnt/extra/models/qwen3.8-27b.gguf"
"#;
    let absent_path = write_temp_toml("kv-per-token-bytes-absent.toml", absent_toml);
    let config = load_config(&absent_path).unwrap();
    let model = config.models.get("qwen3.8:27b").unwrap();
    assert_eq!(model.kv_per_token_bytes(), None);

    // The bare-path shape has no tuning fields at all.
    let bare_toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models]
"qwen3:14b" = "/mnt/extra/models/qwen3-14b.gguf"
"#;
    let bare_path = write_temp_toml("kv-per-token-bytes-bare.toml", bare_toml);
    let config = load_config(&bare_path).unwrap();
    let model = config.models.get("qwen3:14b").unwrap();
    assert_eq!(model.kv_per_token_bytes(), None);
}

// ---------------------------------------------------------------------------
// `g5_probe` (docs/superpowers/evidence/2026-08-16-g5-protocol.md §1: "opt-in
// via `g5_probe = true` ... absent = false")
// ---------------------------------------------------------------------------

/// `g5_probe = true` parses and the accessor reflects it.
#[test]
fn g5_probe_true_parses() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models."qwen3.8:27b"]
path = "/mnt/extra/models/qwen3.8-27b.gguf"
g5_probe = true
"#;
    let path = write_temp_toml("g5-probe-true.toml", toml);
    let config = load_config(&path).unwrap();
    let model = config.models.get("qwen3.8:27b").unwrap();
    assert!(model.g5_probe());
}

/// A table entry that omits `g5_probe` defaults to `false`, and the
/// bare-path shape (which cannot express any opt-in) is `false` too.
#[test]
fn g5_probe_absent_defaults_to_false_for_both_shapes() {
    let toml = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models]
"qwen3:14b" = "/mnt/extra/models/qwen3-14b.gguf"

[models."qwen3.8:27b"]
path = "/mnt/extra/models/qwen3.8-27b.gguf"
"#;
    let path = write_temp_toml("g5-probe-absent.toml", toml);
    let config = load_config(&path).unwrap();
    assert!(!config.models.get("qwen3:14b").unwrap().g5_probe());
    assert!(!config.models.get("qwen3.8:27b").unwrap().g5_probe());
}
