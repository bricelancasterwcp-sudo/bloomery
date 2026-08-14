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
