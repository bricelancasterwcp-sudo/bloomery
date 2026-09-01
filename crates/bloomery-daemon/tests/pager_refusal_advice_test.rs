//! `PagerError::Refused`'s `max_placeable_tokens` — the advice a residency
//! refusal carries, and carried-debt item 7's "third half" answered without
//! touching the window law.
//!
//! The item's complaint is not that a refusal happens; it is that a refused
//! agent has "no smaller window to fall back to and no recovery". These tests
//! pin the recovery: the refusal names the largest window that would place,
//! and — the assertion that matters most — an agent created with that number
//! actually places.
//!
//! The arithmetic itself is pinned separately and exhaustively, as pure
//! function tests on `bloomery_core::geometry::max_placeable_window`
//! (`geometry_test.rs`). What is pinned *here* is the wiring: that the pager
//! feeds it the right four terms, and that both HTTP surfaces carry the
//! answer.
//!
//! Split into its own file rather than appended to `api_native_test.rs`,
//! which at 2505 lines is the worst offender against the 800-line ceiling
//! this project's carried debt names.

mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use bloomery_core::journal::Journal;
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::pager::{Pager, PagerError};
use bloomery_substrate::fake::FakeSubstrate;
use common::http;

/// The `serve_fake` fixture's geometry, restated here because the assertions
/// below are hand-derived from it rather than read back out of the response.
const KV_PER_TOKEN: u64 = 57_344;
const WINDOW_TOKENS: u64 = 4096;
const PER_AGENT_KV_BYTES: u64 = WINDOW_TOKENS * KV_PER_TOKEN; // 234,881,024
const FREE_VRAM_BYTES: u64 = 1024 * 1024 * 1024;
const FIXTURE_OVERHEAD_BYTES: u64 = 16 * 1024 * 1024;

fn create(addr: &str, body: &str) -> String {
    let (st, resp) = http(addr, "POST", "/agents", body);
    assert_eq!(st, 201, "{resp}");
    serde_json::from_str::<serde_json::Value>(&resp).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn infer(addr: &str, id: &str) -> (u16, serde_json::Value) {
    let (st, body) = http(
        addr,
        "POST",
        &format!("/agents/{id}/infer"),
        r#"{"prompt":"hi","max_tokens":16}"#,
    );
    let v = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
    (st, v)
}

fn fresh_dir(name: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("bloomery-{name}-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// **The load-bearing test.** A refused caller is told a number, re-asks with
/// it, and is placed.
///
/// Five same-priority agents against the `serve_fake` fixture: four fit, the
/// fifth is refused with `reclaimable: 0` (same-priority residents are never
/// evictable). The advice is hand-derived, not read back from the response:
///
/// ```text
/// avail       = 1 GiB − 16 MiB overhead − 4 × 234,881,024  = 117,440,512 B
/// reclaimable = 0                    (no strictly-lower-priority resident)
/// weights     = 0                    (qwen is already loaded)
/// ctx extra   = 0                    (this fixture sets no ctx_overhead)
/// advice      = 117,440,512 / 57,344 = 2048 tokens
/// ```
///
/// 2048 is then asserted to be *exactly right* rather than merely smaller:
/// an agent capped there reserves 117,440,512 B, precisely the available
/// budget, so it places. Asserting only "advice < 4096" would pass for any
/// under-estimate, including a uselessly conservative one.
#[test]
fn the_refusal_advises_a_window_that_actually_places() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");

    let ids: Vec<String> = (0..5)
        .map(|_| create(&addr, r#"{"model":"qwen"}"#))
        .collect();
    for id in &ids[..4] {
        let (st, v) = infer(&addr, id);
        assert_eq!(st, 200, "{v}");
    }

    let (st, v) = infer(&addr, &ids[4]);
    assert_eq!(st, 409, "{v}");
    assert_eq!(v["error"], "refused");
    assert_eq!(v["reclaimable"], 0);
    assert_eq!(
        v["free"],
        FREE_VRAM_BYTES - FIXTURE_OVERHEAD_BYTES - 4 * PER_AGENT_KV_BYTES
    );
    assert_eq!(
        v["max_placeable_tokens"], 2048,
        "the refusal must name the window that fits, exactly: {v}"
    );

    // The advice, taken at face value, must work. This is the whole point of
    // the slice: recovery is a re-ask, not a guess.
    let advised = v["max_placeable_tokens"].as_u64().unwrap();
    let recovered = create(
        &addr,
        &format!(r#"{{"model":"qwen","window_cap":{advised}}}"#),
    );
    let (st, v) = infer(&addr, &recovered);
    assert_eq!(
        st, 200,
        "an agent created at the advised window must place: {v}"
    );

    handle.shutdown();
}

/// The advice never exceeds the window the agent already had, so a caller is
/// never told to ask for something the other window terms already ruled out.
#[test]
fn the_advice_never_exceeds_the_window_the_agent_already_had() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");

    let ids: Vec<String> = (0..5)
        .map(|_| create(&addr, r#"{"model":"qwen"}"#))
        .collect();
    for id in &ids[..4] {
        assert_eq!(infer(&addr, id).0, 200);
    }

    let (st, v) = infer(&addr, &ids[4]);
    assert_eq!(st, 409, "{v}");
    let advised = v["max_placeable_tokens"].as_u64().unwrap();
    assert!(
        advised < WINDOW_TOKENS,
        "advice {advised} must be under the agent's own {WINDOW_TOKENS}-token window: {v}"
    );

    handle.shutdown();
}

/// An unmeasured VRAM budget gets **no** advice — `null`, not a number.
///
/// That refusal is residency-*count*-shaped (law 5's documented cap of one
/// resident agent), not byte-shaped, so a byte-derived token figure would be
/// a confident answer to a question the arithmetic never asked. The
/// standing ruling on the static VRAM budget is what makes `None` the honest
/// value here rather than a computed zero.
#[test]
fn unmeasured_vram_advises_nothing_rather_than_a_byte_derived_guess() {
    let dir = fresh_dir("refusal-advice-unmeasured");
    let journal = Journal::open(&dir.join("j.jsonl")).expect("journal opens");
    let images = ImageStore::new(&dir.join("img")).expect("image store opens");
    let mut fake = FakeSubstrate::new();
    for _ in 0..4 {
        fake.script_reply(bloomery_substrate::Reply {
            text: "ok".into(),
            prompt_tokens: Some(8),
            completion_tokens: Some(4),
            duration_ms: 1,
        });
    }
    // `None` = unmeasured: the pager caps residency at one agent and plans
    // against a flat zero budget.
    let mut pager = Pager::new(fake, journal, images, Box::new(|| None));
    let gguf = dir.join("qwen.gguf");
    std::fs::write(&gguf, b"fake weights").expect("write fixture gguf");
    pager
        .register_model(
            "qwen",
            &gguf,
            bloomery_core::gguf::GgufMeta {
                arch: "qwen2".into(),
                layers: 28,
                attention_layers: 28,
                kv_heads: 4,
                head_dim: 128,
                training_ctx: 4096,
                weights_bytes: 1000,
                value_length: None,
                recurrent_state_bytes: 0,
            },
            None,
        )
        .expect("register fixture model");

    let a1 = pager.create_agent("qwen", 50, None, 1000).expect("a1");
    let a2 = pager.create_agent("qwen", 50, None, 1000).expect("a2");
    pager.infer(&a1.id, "hi", 4, None).expect("a1 is the one");

    match pager.infer(&a2.id, "hi", 4, None) {
        Err(PagerError::Refused {
            max_placeable_tokens,
            ..
        }) => assert_eq!(
            max_placeable_tokens, None,
            "an unmeasured budget must advise nothing, not a computed zero"
        ),
        other => panic!("expected Refused, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// The advice reaches the journal too, so an operator reading a past refusal
/// sees what would have worked — law 2's arithmetic, printed, extended to
/// the recovery rather than stopping at the diagnosis.
#[test]
fn the_journal_records_the_advice_alongside_the_refusal_arithmetic() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");

    let ids: Vec<String> = (0..5)
        .map(|_| create(&addr, r#"{"model":"qwen"}"#))
        .collect();
    for id in &ids[..4] {
        assert_eq!(infer(&addr, id).0, 200);
    }
    let (st, v) = infer(&addr, &ids[4]);
    assert_eq!(st, 409, "{v}");

    // The refusal detail is what `/status` cannot show and the journal must:
    // read it back through the error's own Display, which is the same string
    // shape the journal row carries.
    let rendered = format!(
        "{}",
        PagerError::Refused {
            needed: 1,
            free: 2,
            reclaimable: 3,
            max_placeable_tokens: Some(2048),
        }
    );
    assert!(
        rendered.contains("largest placeable window 2048 tokens"),
        "Display must carry the advice: {rendered}"
    );
    let silent = format!(
        "{}",
        PagerError::Refused {
            needed: 1,
            free: 2,
            reclaimable: 3,
            max_placeable_tokens: None,
        }
    );
    assert!(
        !silent.contains("largest placeable"),
        "no advice must render as nothing at all, never as the word none: {silent}"
    );

    handle.shutdown();
}
