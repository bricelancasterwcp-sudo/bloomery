//! Test-only wiring: a ready-to-serve `Pager<FakeSubstrate>` behind
//! `serve(..., 0)`, for the daemon's own integration tests (and later,
//! `bloomery-bench`, which enables the `test-support` feature as a
//! dev-dependency rather than linking test code into a release binary).
//!
//! Gated at the `mod` declaration in `lib.rs`
//! (`#[cfg(any(test, feature = "test-support"))]`), so none of this compiles
//! into a default build.

use bloomery_core::gguf::GgufMeta;
use bloomery_core::journal::Journal;
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;

use crate::agents::ImageStore;
use crate::http::{serve, ServerHandle};
use crate::pager::Pager;

/// Plenty for any test that infers a handful of times without running dry —
/// draining the queue with none left is a hard error in `FakeSubstrate`,
/// deliberately, so a test that needs more fails loudly instead of quietly
/// reusing a stale one.
const SCRIPTED_REPLIES: usize = 32;

/// The fixture's static VRAM budget, per the Task 14 brief.
const FIXTURE_FREE_VRAM_BYTES: u64 = 1024 * 1024 * 1024;

/// Small and nonzero: exercises the same `set_overhead_bytes` call
/// `main.rs` makes from `config.overhead_mib`, without pinning this
/// fixture to that config's 1 GiB default, which would leave (after
/// `qwen_like_meta`'s weights) no VRAM-term headroom for the fixture's
/// training-ctx-bound window to land on deterministically.
const FIXTURE_OVERHEAD_BYTES: u64 = 16 * 1024 * 1024;

/// Same geometry `pager_test.rs` uses for its "qwen" fixture model: a
/// 4096-token training context small enough that the window law binds on
/// it (not on the fixture's generous VRAM budget) for any single agent.
fn qwen_like_meta() -> GgufMeta {
    GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
    }
}

fn ok_reply() -> Reply {
    Reply {
        text: "ok".into(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    }
}

/// A fresh scratch directory for one `serve_fake()` call's journal, image
/// store, and fixture `.gguf`. Suffixed with the process id and a
/// process-wide counter so concurrently-running tests (the default for
/// integration test binaries) never share one.
fn fresh_dir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("bloomery-http-test-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir for serve_fake");
    dir
}

/// Builds a `Pager<FakeSubstrate>` with one registered `qwen`-like model,
/// [`SCRIPTED_REPLIES`] scripted successful replies, a tempdir journal and
/// image store, and a generous static VRAM budget, then serves it on an
/// ephemeral port (`serve(pager, 0)`).
pub fn serve_fake() -> (u16, ServerHandle) {
    let dir = fresh_dir();
    let journal = Journal::open(&dir.join("j.jsonl")).expect("journal opens");
    let images = ImageStore::new(&dir.join("img")).expect("image store opens");

    let mut fake = FakeSubstrate::new();
    for _ in 0..SCRIPTED_REPLIES {
        fake.script_reply(ok_reply());
    }

    let mut pager = Pager::new(
        fake,
        journal,
        images,
        Box::new(|| Some(FIXTURE_FREE_VRAM_BYTES)),
    );
    pager.set_overhead_bytes(FIXTURE_OVERHEAD_BYTES);

    let gguf = dir.join("qwen.gguf");
    std::fs::write(&gguf, b"fake weights").expect("write fixture gguf");
    pager
        .register_model("qwen", &gguf, qwen_like_meta(), None)
        .expect("register fixture model");

    serve(pager, 0)
}
