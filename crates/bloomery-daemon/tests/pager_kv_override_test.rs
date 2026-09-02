//! Part B (spec §10 addendum): the declared `kv_per_token` override.
//!
//! Split out of `pager_weights_test.rs` on 2026-09-01 (carried-debt slice D).

mod common;

use bloomery_daemon::pager::*;
use common::pager::{fresh_dir, meta, pager_in, write_gguf};

use common::pager_weights::MIB;

/// The `meta(1000)` fixture's GGUF-derived `kv_per_token` (28 layers, 4
/// kv-heads, 128 head-dim — the module doc's "qwen" geometry): `2 * 28 * 4 *
/// 128 * 2 = 57_344` B/token. Named here so every test below states its
/// asymmetric numbers against this one constant rather than a magic 57_344
/// repeated silently.
const GGUF_KV_PER_TOKEN: u64 = 57_344;

/// A declared override roughly 4x smaller than [`GGUF_KV_PER_TOKEN`] — the
/// spec's own measured motivation (qwen3.8-27b real KV ≈0.064 MiB/token vs
/// the GGUF-derived-charged 0.254 MiB/token, a ~4x overcount on hybrid-
/// DeltaNet architectures).
const DECLARED_KV_PER_TOKEN: u64 = 14_336;

// ---------------------------------------------------------------------------
// Part B (spec §10 addendum): declared kv_per_token override.
// ---------------------------------------------------------------------------

/// `create_agent`'s window law uses the declared `kv_per_token_bytes`
/// override — not the GGUF-derived `kv_per_token` — as
/// `GeometryInput.kv_per_token` (spec §10 addendum). Asymmetric numbers
/// (declared 14 336 B/token, GGUF-derived 57 344 B/token, free VRAM chosen so
/// the window's bound (`training_ctx` vs `vram`) and exact token count
/// diverge sharply between the two — mirrors
/// `create_agent_window_uses_the_declared_weights_not_the_file`'s template):
/// with declared (14 336) charged, `(100_048_576 - 1_048_576) / 14_336 =
/// 6975` tokens exceeds `training_ctx` (4096), so the window is
/// `training_ctx`-bound at exactly 4096; with the GGUF-derived value (57 344)
/// charged instead, `100_000_000 / 57_344 = 1743` tokens is UNDER
/// `training_ctx`, so the window would instead be `vram`-bound at 1743. No
/// placement call happens in this test at all, so it is sensitive to exactly
/// one charge site: `create_agent`'s `GeometryInput.kv_per_token`.
#[test]
fn create_agent_window_uses_the_declared_kv_per_token_not_the_gguf_value() {
    let dir = fresh_dir("bloomery-pager-kv-tuning-window");
    let weights = MIB;
    let remaining = 100_000_000u64;
    let free_vram = remaining + weights;
    let (mut p, _, _) = pager_in(&dir, 0, Some(free_vram));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();
    p.set_kv_per_token_bytes("qwen", Some(DECLARED_KV_PER_TOKEN))
        .unwrap();

    // No window_cap: the window is purely training_ctx-vs-vram bound, so
    // this is a clean read of which per-token value the geometry math used.
    let a1 = p.create_agent("qwen", 50, None, 10_000).unwrap();

    assert_eq!(
        a1.window_tokens, 4096,
        "training_ctx (4096) must win over vram (6975 tokens at declared 14336 B/token) \
         — if the geometry read the GGUF-derived 57344 B/token instead, vram (1743) would win"
    );
    assert_eq!(a1.bound_by, "training_ctx");
}

/// The `kv_bytes` reservation charge — the demand side of `place` AND the
/// supply side (`resident_reserved_bytes`, via the stored `Agent.kv_bytes`)
/// — ALSO uses the declared `kv_per_token_bytes` override, via a SECOND,
/// independent read from the one just pinned above (spec §10 addendum: "one
/// source, both places" — mirrors `placement_uses_the_declared_weights_not_
/// the_file`'s "each independent charge site" property).
///
/// A fixed `window_cap` of 2048 tokens, and a free-VRAM/budget figure large
/// enough that the vram term — computed correctly, from the DECLARED
/// per-token figure, at the window-law site this test does not touch —
/// still exceeds 2048 tokens (`(40_894_464) / 14_336 = 2853 > 2048`), so
/// `window.tokens` is deterministically `2048` (`user_cap`-bound) regardless
/// of any bug at the reservation site under test here. The SAME free-VRAM
/// figure is also the placement budget (40 MiB): it sits strictly between
/// `kv_bytes` computed from the declared figure (`2048 * 14_336 = 28.0 MiB`,
/// demand ≈ 29.0 MiB with weights — fits) and `kv_bytes` computed from the
/// GGUF-derived figure (`2048 * 57_344 = 112.0 MiB`, demand ≈ 113.0 MiB —
/// would refuse), so the single `infer` call below only succeeds if the
/// reservation charge read the declared value.
#[test]
fn placement_uses_the_declared_kv_per_token_not_the_gguf_value() {
    let dir = fresh_dir("bloomery-pager-kv-tuning-placement");
    let weights = MIB;
    let free_vram = 40 * MIB; // budget == free_vram: pager_in's one closure serves both.
    let (mut p, _, _) = pager_in(&dir, 1, Some(free_vram));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();
    p.set_kv_per_token_bytes("qwen", Some(DECLARED_KV_PER_TOKEN))
        .unwrap();

    let a1 = p.create_agent("qwen", 50, Some(2048), 10_000).unwrap();
    assert_eq!(
        a1.window_tokens, 2048,
        "user_cap must bind here (see doc comment), so kv_bytes below is deterministic"
    );

    p.infer(&a1.id, "hello", 16, None).expect(
        "demand = weights (1 MiB) + kv_bytes (2048 * declared 14336 B/token ≈ 28.0 MiB) ≈ \
         29.0 MiB, under the 40 MiB budget — if the reservation charge read the GGUF-derived \
         57344 B/token instead, demand (≈113.0 MiB) would blow well past the budget and refuse",
    );
}

/// `/status`'s `kv_per_token` reports the declared override, and
/// `kv_per_token_declared` is `true` — spec §10's naming rule: "a declared
/// number must never read as a measured one" restated the other way around
/// for `/status`.
#[test]
fn status_reports_declared_kv_per_token_and_the_declared_flag_when_override_active() {
    let dir = fresh_dir("bloomery-pager-kv-tuning-status-declared");
    let (mut p, _, _) = pager_in(&dir, 0, Some(1000 * MIB));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(10 * MIB), None)
        .unwrap();
    p.set_kv_per_token_bytes("qwen", Some(DECLARED_KV_PER_TOKEN))
        .unwrap();

    let status = p.status();
    let model = &status.models[0];
    assert_eq!(model.kv_per_token, DECLARED_KV_PER_TOKEN);
    assert!(
        model.kv_per_token_declared,
        "a declared override must be flagged, never silently read as measured"
    );
}

/// The mirror: no override at all reports the GGUF-derived value, and the
/// declared flag is `false` — never a confident-looking declared reading for
/// a number nobody actually declared.
#[test]
fn status_reports_gguf_kv_per_token_and_no_declared_flag_when_absent() {
    let dir = fresh_dir("bloomery-pager-kv-tuning-status-absent");
    let (mut p, _, _) = pager_in(&dir, 0, Some(1000 * MIB));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(10 * MIB), None)
        .unwrap();
    // No set_kv_per_token_bytes call at all.

    let status = p.status();
    let model = &status.models[0];
    assert_eq!(model.kv_per_token, GGUF_KV_PER_TOKEN);
    assert!(
        !model.kv_per_token_declared,
        "no override means the value is measured (GGUF-derived), not declared"
    );
}

/// A declared value LARGER than the GGUF-derived figure is NOT clamped
/// (spec §10 addendum: "No clamp against the GGUF value... a declared value
/// larger than GGUF is allowed (extra conservative), smaller is the point")
/// — unlike `weights_vram_mib`'s `min(declared, file)`. `/status` reports
/// the declared value raw, larger than the GGUF-derived one.
#[test]
fn a_declared_kv_per_token_larger_than_gguf_is_not_clamped() {
    let dir = fresh_dir("bloomery-pager-kv-tuning-no-clamp");
    let (mut p, _, _) = pager_in(&dir, 0, Some(1000 * MIB));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(10 * MIB), None)
        .unwrap();
    let larger_declared = GGUF_KV_PER_TOKEN * 3;
    p.set_kv_per_token_bytes("qwen", Some(larger_declared))
        .unwrap();

    let status = p.status();
    let model = &status.models[0];
    assert_eq!(
        model.kv_per_token, larger_declared,
        "unlike weights, a larger declared kv_per_token is used as-is, not clamped down"
    );
    assert!(model.kv_per_token_declared);
}

/// `set_kv_per_token_bytes` on an unregistered model name is refused with
/// `UnknownModel`, naming the model — same contract as `set_model_tuning`.
#[test]
fn set_kv_per_token_bytes_on_unknown_model_is_refused() {
    let dir = fresh_dir("bloomery-pager-kv-tuning-unknown");
    let (mut p, _, _) = pager_in(&dir, 0, Some(300 * MIB));
    match p.set_kv_per_token_bytes("nope", Some(1)) {
        Err(PagerError::UnknownModel(name)) => assert_eq!(name, "nope"),
        other => panic!("expected UnknownModel, got {other:?}"),
    }
}
