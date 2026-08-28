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
use crate::http::{serve, serve_shared_with_memory, ServerHandle};
use crate::memory::MemoryContext;
use crate::pager::Pager;

/// Plenty for any test that infers a handful of times without running dry —
/// draining the queue with none left is a hard error in `FakeSubstrate`,
/// deliberately, so a test that needs more fails loudly instead of quietly
/// reusing a stale one.
const SCRIPTED_REPLIES: usize = 32;

/// The fixture's static VRAM budget, per the Task 14 brief.
///
/// Task 3 bumped this by `qwen_like_meta().weights_bytes` (1000 B): weights
/// now enter the reservation budget too, so once a request loads `qwen` that
/// many bytes are permanently charged against `avail` for every placement
/// after it. The `+ 1000` keeps every byte-exact `free`/`needed` assertion
/// pinned against this fixture (e.g.
/// `api_native_test.rs::infer_residency_refusal_returns_409_with_arithmetic`)
/// unchanged rather than rederiving them around the new weights term.
const FIXTURE_FREE_VRAM_BYTES: u64 = 1024 * 1024 * 1024 + 1000;

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
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
        value_length: None,
        recurrent_state_bytes: 0,
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

/// Builds the fixture `Pager<FakeSubstrate>` both [`serve_fake`] and
/// [`fake_pager_for_v1`] share: one registered `qwen`-like model,
/// [`SCRIPTED_REPLIES`] scripted successful replies, a tempdir journal and
/// image store, and a generous static VRAM budget. Returns the scratch dir
/// alongside it — the caller decides whether that's a `ServerHandle`'s to
/// clean up or its own.
fn build_fake_pager() -> (std::path::PathBuf, Pager<FakeSubstrate>) {
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

    (dir, pager)
}

/// Builds a `Pager<FakeSubstrate>` with one registered `qwen`-like model,
/// [`SCRIPTED_REPLIES`] scripted successful replies, a tempdir journal and
/// image store, and a generous static VRAM budget, then serves it on an
/// ephemeral port (`serve(pager, 0)`).
pub fn serve_fake() -> (u16, ServerHandle) {
    let (dir, pager) = build_fake_pager();
    serve_with_cleanup(dir, pager)
}

/// [`serve_fake`] with the pager's request defaults set, standing in for a
/// daemon booted from a config carrying `default_priority` /
/// `default_budget_tokens` — the only way to tell a carried config value
/// from a constant retyped in the HTTP layer.
pub fn serve_fake_with_defaults(priority: u8, budget_tokens: u64) -> (u16, ServerHandle) {
    let (dir, mut pager) = build_fake_pager();
    pager.set_defaults(priority, budget_tokens);
    serve_with_cleanup(dir, pager)
}

/// [`serve_fake`] with an operator-declared tier, standing in for
/// `main.rs`'s `config.tier` wiring.
pub fn serve_fake_with_tier(name: &str, emulated: bool) -> (u16, ServerHandle) {
    let (dir, mut pager) = build_fake_pager();
    pager.set_tier(name, emulated);
    serve_with_cleanup(dir, pager)
}

/// [`serve_fake`] served through [`serve_shared_with_memory`] instead of
/// plain [`serve`] — same fixture pager, but with a real memory organ
/// context wired in, for `/status`'s `memory` object and the task surface's
/// retrieve/mint pipeline. The caller builds `memory` itself (typically via
/// `memory::build_memory`) rather than this helper building one from a
/// config the caller has no way to hand in here — the tests that need this
/// (forcing a load failure via a directory at the store path, e.g.) need
/// control of `memory`'s own construction anyway.
pub fn serve_fake_with_memory(memory: std::sync::Arc<MemoryContext>) -> (u16, ServerHandle) {
    let (dir, pager) = build_fake_pager();
    let (port, mut handle) =
        serve_shared_with_memory(std::sync::Arc::new(std::sync::Mutex::new(pager)), 0, memory);
    handle.set_scratch_dir(dir);
    (port, handle)
}

fn serve_with_cleanup(dir: std::path::PathBuf, pager: Pager<FakeSubstrate>) -> (u16, ServerHandle) {
    let (port, mut handle) = serve(pager, 0);
    // Without this, every `serve_fake*()` call litters the OS tempdir with a
    // journal/image/fixture directory that nothing else ever removes — a
    // stale `bloomery-http-test-*` per test run, forever.
    handle.set_scratch_dir(dir);
    (port, handle)
}

/// The same fixture as [`serve_fake`], but not served over a socket — it
/// stays a bare `Mutex<Pager<FakeSubstrate>>`, the type every dispatch
/// function in this crate already takes.
///
/// Exists for exactly one thing `serve_fake` cannot do: driving several
/// `/v1` requests against the *same* pager and then inspecting
/// `FakeSubstrate::ctx_history` on it afterward. Once a pager is handed to
/// `http::serve` it is moved behind a socket-serving `Arc` with no way back
/// out — there is no `ServerHandle::into_pager`, deliberately, since the
/// real daemon never wants one either. The returned scratch dir is the
/// caller's to remove (`std::fs::remove_dir_all`, best-effort) — there is
/// no `ServerHandle` here to register it with.
pub fn fake_pager_for_v1() -> (std::path::PathBuf, std::sync::Mutex<Pager<FakeSubstrate>>) {
    let (dir, pager) = build_fake_pager();
    (dir, std::sync::Mutex::new(pager))
}

/// A `serve_fake()` variant for Task 5's task HTTP surface: scripts
/// `<action>`-shaped `replies` (FIFO) instead of the generic `"ok"` text
/// every other `serve_fake*` variant scripts (a plain "ok" reply doesn't
/// parse as an `<action>` envelope, so `run_task` would just re-ask twice
/// and fail every step), wires `tasks_enabled` and a baseline
/// `ExecBounds::default()`, and points `Pager::set_task_journal_path` at a
/// `tasks.jsonl` inside the fixture's own scratch dir.
///
/// Returns a `sandbox` directory (already created, inside the scratch dir
/// `ServerHandle::shutdown` cleans up) for a caller to write fixture files
/// into and scope a `Grant` to — mirroring `task_loop_test.rs::sandbox`,
/// just reachable over HTTP instead of driving `run_task` directly.
pub fn serve_fake_with_tasks(
    tasks_enabled: bool,
    replies: Vec<Reply>,
) -> (u16, ServerHandle, std::path::PathBuf) {
    let dir = fresh_dir();
    let journal = Journal::open(&dir.join("j.jsonl")).expect("journal opens");
    let images = ImageStore::new(&dir.join("img")).expect("image store opens");

    let mut fake = FakeSubstrate::new();
    for r in replies {
        fake.script_reply(r);
    }

    let mut pager = Pager::new(
        fake,
        journal,
        images,
        Box::new(|| Some(FIXTURE_FREE_VRAM_BYTES)),
    );
    pager.set_overhead_bytes(FIXTURE_OVERHEAD_BYTES);
    pager.set_tasks_enabled(tasks_enabled);
    pager.set_exec_bounds(crate::task::ExecBounds::default());
    pager.set_task_journal_path(dir.join("tasks.jsonl"));

    let gguf = dir.join("qwen.gguf");
    std::fs::write(&gguf, b"fake weights").expect("write fixture gguf");
    pager
        .register_model("qwen", &gguf, qwen_like_meta(), None)
        .expect("register fixture model");

    let sandbox = dir.join("sandbox");
    std::fs::create_dir_all(&sandbox).expect("sandbox dir");

    let (port, handle) = serve_with_cleanup(dir, pager);
    (port, handle, sandbox)
}

/// Drives one `/v1` request through the exact `api_v1::dispatch` the real
/// server calls, without opening a socket — the in-process counterpart to
/// driving `serve_fake()` over `tests/common::http`. `path` is the full
/// `/v1/...` path; `agent_header` stands in for an `X-Bloomery-Agent`
/// header, the one header this crate's `/v1` surface reads.
pub fn dispatch_v1_fake(
    pager: &std::sync::Mutex<Pager<FakeSubstrate>>,
    method: &str,
    path: &str,
    body: &str,
    agent_header: Option<&str>,
) -> (u16, String) {
    let segments: Vec<String> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let result = crate::api_v1::dispatch(pager, method, &segments, body, agent_header);
    let body = match result.body {
        crate::api_v1::V1Body::Json(v) => v.to_string(),
        crate::api_v1::V1Body::Sse(s) => s,
    };
    (result.status, body)
}
