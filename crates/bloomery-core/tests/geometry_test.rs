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
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 32768,
        weights_bytes: 0,
    };
    assert_eq!(kv_bytes_per_token(&m), 57344); // 56 KiB — robigo's measured row
}
