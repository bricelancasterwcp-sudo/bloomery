//! Live llama.cpp substrate test: lifecycle and the stats contract.
//!
//! This and `llama_semantic_test.rs` are the only tests in the workspace that
//! need a GPU, a real GGUF and the `llama` feature; everything else stays
//! GPU-free. Both are `#[ignore]`d *and* env-gated so neither `cargo test` nor
//! `cargo test -- --ignored` can run them by accident.
//!
//! Requires: `BLOOMERY_LIVE=1`, `BLOOMERY_TEST_GGUF=/path/to/model.gguf`,
//! and `--features llama` (add `,vulkan` for GPU offload).
//!
//! The semantic test lives in a *separate* file on purpose: `LlamaBackend`
//! initialisation is process-global, so two live tests in one binary race and
//! the loser gets `BackendAlreadyInitialized`. Cargo runs each integration
//! test binary as its own process, sequentially, which sidesteps that (and
//! frees the model's VRAM between the two).
#![cfg(feature = "llama")]

#[test]
#[ignore]
fn live_infer_reports_stats_and_state_round_trips() {
    if std::env::var("BLOOMERY_LIVE").as_deref() != Ok("1") {
        // Say so out loud: an env-gated test that silently returns reads as a
        // pass, and a green suite that never ran the GPU is a false green.
        eprintln!("SKIPPED: BLOOMERY_LIVE unset");
        return;
    }
    use bloomery_substrate::{llama::LlamaSubstrate, Substrate};
    let gguf = std::env::var("BLOOMERY_TEST_GGUF").expect("set BLOOMERY_TEST_GGUF");
    let mut s = LlamaSubstrate::new().unwrap();
    let m = s.load_model(std::path::Path::new(&gguf), 99).unwrap();
    let c = s.create_context(m, 2048).unwrap();
    let r = s.infer(c, "Reply with exactly: OK", 8).unwrap();
    assert!(
        r.prompt_tokens.is_some() && r.completion_tokens.is_some(),
        "real counts by construction — we count decoded tokens ourselves"
    );
    let img = s.save_state(c).unwrap();
    assert!(!img.is_empty());
    s.destroy_context(c).unwrap();
    let c2 = s.create_context(m, 2048).unwrap();
    s.load_state(c2, &img).unwrap();
    let r2 = s.infer(c2, " Again: OK", 8).unwrap(); // continues on restored KV
    assert!(r2.completion_tokens.unwrap() > 0);
}
