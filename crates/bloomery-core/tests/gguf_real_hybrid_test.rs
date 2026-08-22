//! Parses the real REAP-48-ours GGUF when it is on this box (turn-5 spec §2)
//! and pins the two derived hybrid numbers against the spike's measurements.
//! Skips (prints, passes) when the file is absent, so CI never depends on it.
use bloomery_core::geometry::kv_bytes_per_token;
use bloomery_core::gguf::parse_gguf_meta;

#[test]
fn reap48_ours_gguf_derives_the_measured_hybrid_geometry() {
    let path = std::env::var("BLOOMERY_HYBRID_GGUF").unwrap_or_else(|_| {
        format!(
            "{}/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    let path = std::path::Path::new(&path);
    if !path.exists() {
        eprintln!("skipped: {} not present", path.display());
        return;
    }
    let m = parse_gguf_meta(path).expect("the real GGUF parses");
    assert_eq!(m.arch, "qwen35moe");
    assert_eq!((m.layers, m.attention_layers), (40, 10));
    assert_eq!(
        kv_bytes_per_token(&m),
        20_480,
        "llama.cpp: 1070.00 MiB / 54,784 cells"
    );
    assert_eq!(
        m.recurrent_state_bytes, 65_863_680,
        "llama.cpp: RS buffer 62.81 MiB"
    );
    assert_eq!(m.training_ctx, 262_144);
}
