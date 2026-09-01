use bloomery_core::geometry::*;

const KIB: u64 = 1024;
const GIB: u64 = 1024 * 1024 * 1024;

fn base() -> GeometryInput {
    GeometryInput {
        training_ctx: 32768,
        kv_per_token: 56 * KIB, // qwen2.5-coder-7b, measured (robigo)
        weights_bytes: 8 * GIB,
        free_vram_bytes: Some(14 * GIB),
        overhead_bytes: GIB,
        // Zero here, deliberately: every test built on `base()` below is
        // the backward-equivalence property (item 7, task 1(b)) — with
        // ctx_overhead_bytes at 0 every pre-fix expectation must still
        // hold byte for byte.
        ctx_overhead_bytes: 0,
        user_cap: None,
        measured_ceiling: None,
    }
}

#[test]
fn training_ctx_binds_when_vram_is_ample() {
    let w = usable_window(&base());
    assert_eq!(w.tokens, 32768);
    assert_eq!(w.bound_by, BoundBy::TrainingCtx);
    assert!(!w.vram_unmeasured);
}

#[test]
fn vram_binds_when_scarce() {
    let mut i = base();
    i.free_vram_bytes = Some(4 * GIB);
    i.weights_bytes = 2 * GIB;
    // (4 - 2 - 1) GiB / 56 KiB = 18724 tokens
    let w = usable_window(&i);
    assert_eq!(w.tokens, 18724);
    assert_eq!(w.bound_by, BoundBy::Vram);
}

#[test]
fn user_cap_binds() {
    let mut i = base();
    i.user_cap = Some(8192);
    let w = usable_window(&i);
    assert_eq!((w.tokens, w.bound_by), (8192, BoundBy::UserCap));
}

#[test]
fn measured_ceiling_binds_below_everything() {
    let mut i = base();
    i.measured_ceiling = Some(11500);
    let w = usable_window(&i);
    assert_eq!((w.tokens, w.bound_by), (11500, BoundBy::MeasuredCeiling));
}

#[test]
fn unmeasured_vram_is_flagged_not_zeroed() {
    let mut i = base();
    i.free_vram_bytes = None;
    let w = usable_window(&i);
    assert_eq!(w.tokens, 32768); // other terms still apply
    assert!(w.vram_unmeasured); // law 5: named, never silently defaulted
}

#[test]
fn vram_term_saturates_instead_of_wrapping_and_ties_favor_training_ctx() {
    // A units-bug upstream (e.g. reporting bits instead of bytes) could hand
    // us a huge free_vram_bytes / tiny kv_per_token combination whose raw
    // quotient exceeds u32::MAX. The Vram candidate must saturate to
    // u32::MAX rather than wrap, and since training_ctx is also u32::MAX
    // here, the two candidates tie on tokens — the earlier-declared term
    // (TrainingCtx) must win the tie, never the later one.
    //
    // free_vram_bytes = 2^32 is the deliberately discriminating input: the
    // raw quotient is exactly 4_294_967_296, one past u32::MAX. A truncating
    // `as u32` cast wraps that to 0 (bound_by = Vram, tokens = 0); the
    // saturating fix instead clamps it to u32::MAX, which then ties with
    // training_ctx and correctly resolves to TrainingCtx by the tie rule.
    // (An earlier version of this test used free_vram_bytes = u64::MAX,
    // whose low 32 bits are coincidentally all 1s — i.e. `u64::MAX as u32
    // == u32::MAX` too — so it passed under both the buggy and fixed code
    // and did not actually guard the regression.)
    let i = GeometryInput {
        training_ctx: u32::MAX,
        kv_per_token: 1,
        weights_bytes: 0,
        free_vram_bytes: Some(4_294_967_296), // 2^32
        overhead_bytes: 0,
        ctx_overhead_bytes: 0,
        user_cap: None,
        measured_ceiling: None,
    };
    let w = usable_window(&i);
    assert_eq!(w.tokens, u32::MAX);
    assert_eq!(w.bound_by, BoundBy::TrainingCtx);
}

/// **Item 7, task 1(a).** The VRAM term must charge `ctx_overhead_bytes`
/// alongside `weights_bytes` and `overhead_bytes` — placement already
/// charges it (`Agent::reserved_bytes`), so a window that omitted it was
/// sized to consume memory it could never actually get. Old code (no
/// `ctx_overhead_bytes` subtraction) would compute `(1000 - 400 - 100) / 1 =
/// 500` tokens; this fix requires `(1000 - 400 - 100 - 200) / 1 = 300`.
#[test]
fn vram_term_charges_ctx_overhead() {
    let i = GeometryInput {
        training_ctx: 1_000_000, // huge: must not bind
        kv_per_token: 1,
        weights_bytes: 400,
        free_vram_bytes: Some(1000),
        overhead_bytes: 100,
        ctx_overhead_bytes: 200,
        user_cap: None,
        measured_ceiling: None,
    };
    let w = usable_window(&i);
    assert_eq!(w.tokens, 300);
    assert_eq!(w.bound_by, BoundBy::Vram);
}

/// **Item 7, task 1(c).** `ctx_overhead_bytes` larger than what's left after
/// `weights_bytes` and `overhead_bytes` must saturate to a 0-token window,
/// never panic or wrap: `1000 - 400 - 100 = 500`, and `ctx_overhead_bytes =
/// 600` exceeds that remainder entirely.
#[test]
fn ctx_overhead_larger_than_remainder_saturates_to_zero_without_panicking() {
    let i = GeometryInput {
        training_ctx: 1000,
        kv_per_token: 1,
        weights_bytes: 400,
        free_vram_bytes: Some(1000),
        overhead_bytes: 100,
        ctx_overhead_bytes: 600,
        user_cap: None,
        measured_ceiling: None,
    };
    let w = usable_window(&i);
    assert_eq!(w.tokens, 0);
    assert_eq!(w.bound_by, BoundBy::Vram);
}

#[test]
fn zero_kv_per_token_skips_vram_term_without_panicking() {
    // kv_per_token == 0 makes the VRAM division undefined; zero cost per
    // token means the VRAM term imposes no real constraint, so it must be
    // skipped entirely rather than panicking or reporting a zero window.
    // VRAM was still measured, so vram_unmeasured stays false.
    let mut i = base();
    i.kv_per_token = 0;
    i.free_vram_bytes = Some(GIB);
    let w = usable_window(&i);
    assert_eq!(w.tokens, 32768);
    assert_eq!(w.bound_by, BoundBy::TrainingCtx);
    assert!(!w.vram_unmeasured);
}

#[test]
fn kv_arithmetic_matches_measured_qwen() {
    use bloomery_core::gguf::GgufMeta;
    let m = GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 32768,
        weights_bytes: 0,
        recurrent_state_bytes: 0,
        value_length: None,
    };
    assert_eq!(kv_bytes_per_token(&m), 57344); // 56 KiB — robigo's measured row
}

#[test]
fn kv_bytes_per_token_counts_attention_layers_only() {
    use bloomery_core::gguf::GgufMeta;
    let hybrid = GgufMeta {
        arch: "qwen35moe".into(),
        layers: 40,
        attention_layers: 10,
        kv_heads: 2,
        head_dim: 256,
        training_ctx: 262_144,
        weights_bytes: 11_755_624_288,
        recurrent_state_bytes: 65_863_680,
        value_length: None,
    };
    assert_eq!(kv_bytes_per_token(&hybrid), 20_480, "2 * 10 * 2 * 256 * 2");
    let dense = GgufMeta {
        attention_layers: 40,
        ..hybrid.clone()
    };
    assert_eq!(
        kv_bytes_per_token(&dense),
        81_920,
        "the pre-fix over-count, for the record"
    );
}

// ---------------------------------------------------------------------------
// R9 — MLA, separate K/V widths (gguf-geometry SPEC.md). Measured branch H-b:
// ollama 0.32.13 allocates K at key_length width and V at value_length width
// independently; assay docs/superpowers/evidence/mla-kv-2026-08-27/.
// ---------------------------------------------------------------------------

/// deepseek2-shaped: 27 attention_layers * 16 kv_heads * (192 K + 128 V) * 2
/// f16 bytes = 276,480 B/token — the exact figure Phase 1 measured on ollama
/// 0.32.13, K and V independently reproduced (165,888 + 110,592).
#[test]
fn kv_bytes_per_token_mla_separate_widths_r9() {
    use bloomery_core::gguf::GgufMeta;
    let m = GgufMeta {
        arch: "deepseek2".into(),
        layers: 27,
        attention_layers: 27,
        kv_heads: 16,
        head_dim: 192,
        training_ctx: 163_840,
        weights_bytes: 0,
        recurrent_state_bytes: 0,
        value_length: Some(128),
    };
    assert_eq!(
        kv_bytes_per_token(&m),
        276_480,
        "R9: 27 * 16 * (192 + 128) * 2"
    );
}

/// `value_length == head_dim` stays dense by identity: `head_dim + head_dim
/// == 2 * head_dim`, so the R9 branch (were it to fire) would reproduce the
/// pre-R9 figure exactly. This pins that the dense figure is unchanged
/// whether or not the branch actually fires on an equal-widths meta.
#[test]
fn kv_bytes_per_token_equal_widths_stays_dense() {
    use bloomery_core::gguf::GgufMeta;
    let m = GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 32768,
        weights_bytes: 0,
        recurrent_state_bytes: 0,
        value_length: Some(128),
    };
    assert_eq!(
        kv_bytes_per_token(&m),
        57344,
        "value_length == head_dim: unchanged from the dense figure"
    );
}

/// `value_length: None` (unstated — a pre-R9 file, or a file that simply
/// never states V's width) reads as the dense identity: the pre-R9 formula,
/// byte for byte. `kv_arithmetic_matches_measured_qwen` above already pins
/// this for the same fixture; this test names the requirement explicitly.
#[test]
fn kv_bytes_per_token_unstated_value_length_stays_dense() {
    use bloomery_core::gguf::GgufMeta;
    let m = GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 32768,
        weights_bytes: 0,
        recurrent_state_bytes: 0,
        value_length: None,
    };
    assert_eq!(
        kv_bytes_per_token(&m),
        57344,
        "value_length: None must not change the pre-R9 dense figure"
    );
}

// ---------------------------------------------------------------------------
// `max_placeable_window` — the advice a residency refusal carries.
//
// Slice C (2026-09-01) answers carried-debt item 7's third half by making the
// refusal actionable rather than by changing the window law. The refused
// caller is told the largest window that WOULD place, so recovery is a
// mechanical re-ask instead of a guess. The function is pure so the honesty
// rules below are pinned without a pager.
// ---------------------------------------------------------------------------

/// The ordinary case: headroom minus the two non-KV charges, divided by the
/// per-token cost. 100 MiB of headroom, 20 MiB of per-context reservation
/// beyond KV, nothing to load, 1 MiB/token -> 80 tokens.
#[test]
fn advises_the_tokens_that_actually_fit() {
    let mib = 1024 * 1024;
    assert_eq!(
        max_placeable_window(100 * mib, 0, 20 * mib, mib, 4096),
        Some(80)
    );
}

/// A cold model must fit its weights too — they are part of what the
/// placement will charge, so they come off the headroom before the division.
#[test]
fn a_cold_models_weights_come_off_the_headroom_first() {
    let mib = 1024 * 1024;
    assert_eq!(
        max_placeable_window(100 * mib, 50 * mib, 20 * mib, mib, 4096),
        Some(30)
    );
}

/// Never advise more than the agent already had. The caller is recovering
/// from a refusal; advice above its own window would be advice to ask for
/// something the other window terms (training_ctx, user_cap,
/// measured_ceiling) already ruled out.
#[test]
fn the_advice_is_clamped_to_the_window_the_agent_already_had() {
    let mib = 1024 * 1024;
    assert_eq!(
        max_placeable_window(10_000 * mib, 0, 0, mib, 512),
        Some(512),
        "9999 tokens of room must not become advice to ask for 9999"
    );
}

/// Zero is a real answer, not a missing one: nothing places even with every
/// eligible resident evicted. Saying `Some(0)` is the honest form — the
/// caller learns there is no window to retry with, which is different from
/// "we could not work it out".
#[test]
fn zero_is_advised_when_nothing_fits_at_all() {
    let mib = 1024 * 1024;
    assert_eq!(
        max_placeable_window(10 * mib, 0, 20 * mib, mib, 4096),
        Some(0)
    );
}

/// `kv_per_token == 0` is the one case with no answer: with no per-token
/// cost there is no VRAM-bound window to advise, and dividing by it is
/// undefined. `None`, never a confident zero — the same rule
/// `usable_window` follows when it skips the Vram candidate entirely.
#[test]
fn no_advice_when_a_token_costs_nothing() {
    assert_eq!(max_placeable_window(1024, 0, 0, 0, 4096), None);
}

/// The subtractions saturate rather than wrap: charges larger than the
/// headroom mean nothing fits, which is `Some(0)`, not a huge window from a
/// u64 underflow.
#[test]
fn oversized_charges_saturate_to_zero_rather_than_wrapping() {
    let mib = 1024 * 1024;
    assert_eq!(
        max_placeable_window(mib, 500 * mib, 500 * mib, 1024, 4096),
        Some(0)
    );
}

/// A quotient beyond `u32` is clamped by the window cap like any other, so
/// the `u64 -> u32` narrowing can never truncate into a small, wrong number.
#[test]
fn an_enormous_quotient_is_capped_not_truncated() {
    assert_eq!(max_placeable_window(u64::MAX, 0, 0, 1, 4096), Some(4096));
}

/// The division truncates DOWN, and a remainder is where that shows.
///
/// Every other case here divides evenly, which makes truncation and rounding
/// indistinguishable — a mutation check (2026-09-01) caught exactly that:
/// swapping `/` for `div_ceil` left all of them green. Rounding up would
/// advise a window whose reservation exceeds the headroom by the remainder,
/// so the caller retries and is refused a second time. One honest refusal
/// must not become two.
#[test]
fn a_remainder_truncates_down_never_up() {
    let mib = 1024 * 1024;
    assert_eq!(
        max_placeable_window(100 * mib + 1000, 0, 0, mib, 4096),
        Some(100),
        "1000 B of remainder must not buy a 101st token"
    );
}
