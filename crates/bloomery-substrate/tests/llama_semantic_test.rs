//! Live llama.cpp substrate test: does a restored KV image really carry the
//! conversation?
//!
//! `llama_live_test.rs` checks lifecycle and the stats contract, but it would
//! pass for a substrate that returned garbage — it only asserts that stats
//! exist and that *something* was generated. This file plants a fact that
//! cannot be guessed, pages the context out and back into a different
//! context, and asks for the fact back, with a cold context as the control.
//!
//! Requires: `BLOOMERY_LIVE=1`, `BLOOMERY_TEST_GGUF=/path/to/model.gguf`, and
//! `--features llama` (add `,vulkan` for GPU offload). It prompts in ChatML,
//! so point `BLOOMERY_TEST_GGUF` at a ChatML instruct model
//! (qwen2.5-coder-7b-instruct-q8_0 is what it was written against).
//!
//! Separate binary from `llama_live_test.rs` on purpose: `LlamaBackend` init
//! is process-global, so two live tests sharing a process race for it.
#![cfg(feature = "llama")]

/// Plant an unguessable fact, page the context out, page it back into a new
/// context, and ask for the fact — with a cold context as the control.
#[test]
#[ignore]
fn live_state_restore_preserves_semantics() {
    if std::env::var("BLOOMERY_LIVE").as_deref() != Ok("1") {
        return;
    }
    use bloomery_substrate::{llama::LlamaSubstrate, Substrate};

    const PLANT: &str = "<|im_start|>user\nRemember this code word exactly: BLOOMERY-7. \
                         Reply with just: OK<|im_end|>\n<|im_start|>assistant\n";
    const RECALL: &str = "<|im_end|>\n<|im_start|>user\nWhat was the code word? \
                          Reply with just the code word.<|im_end|>\n<|im_start|>assistant\n";
    const SECRET: &str = "BLOOMERY-7";

    let gguf = std::env::var("BLOOMERY_TEST_GGUF").expect("set BLOOMERY_TEST_GGUF");
    let mut s = LlamaSubstrate::new().unwrap();
    let m = s.load_model(std::path::Path::new(&gguf), 99).unwrap();

    // Plant the fact, then page the context out.
    let c1 = s.create_context(m, 2048).unwrap();
    let planted = s.infer(c1, PLANT, 24).unwrap();
    assert!(planted.prompt_tokens.unwrap() > 0);
    let img = s.save_state(c1).unwrap();
    assert!(
        img.len() > 1024,
        "a real KV image, not a stub: got {} bytes",
        img.len()
    );
    s.destroy_context(c1).unwrap();

    // Page it back into a brand-new context and ask for the fact.
    let c2 = s.create_context(m, 2048).unwrap();
    s.load_state(c2, &img).unwrap();
    let recalled = s.infer(c2, RECALL, 24).unwrap();
    assert!(
        recalled.text.contains(SECRET),
        "restored context lost the conversation: {:?}",
        recalled.text
    );
    assert!(recalled.completion_tokens.unwrap() > 0);

    // Control: the same question with no restored state cannot answer it, so
    // the recall above came from the image and not from the prompt.
    let cold = s.create_context(m, 2048).unwrap();
    let guessed = s.infer(cold, RECALL, 24).unwrap();
    assert!(
        !guessed.text.contains(SECRET),
        "cold context knew the secret — the recall proves nothing: {:?}",
        guessed.text
    );

    // A damaged image is refused as "size mismatch", which is Task 13's cue to
    // cold-start rather than crash.
    let c3 = s.create_context(m, 2048).unwrap();
    let mut truncated = img.clone();
    truncated.truncate(img.len() / 2);
    let err = format!("{:?}", s.load_state(c3, &truncated).unwrap_err());
    assert!(err.contains("size mismatch"), "{err}");
    let err = format!("{:?}", s.load_state(c3, &[]).unwrap_err());
    assert!(err.contains("size mismatch"), "{err}");

    // Law 2: refuse rather than truncate. The window is read back from
    // llama.cpp (which pads it), never assumed from the request.
    let small = s.create_context(m, 64).unwrap();
    let err = format!("{:?}", s.infer(small, &"word ".repeat(400), 8).unwrap_err());
    assert!(err.contains("refusing"), "{err}");

    // Unloading a model with live contexts must drop contexts first.
    s.unload_model(m).unwrap();
}
