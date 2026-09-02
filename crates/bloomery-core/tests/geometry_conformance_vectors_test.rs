//! gguf-geometry contract conformance, one `#[test]` per vendored vector.
//!
//! One test per vector rather than a loop, deliberately: a failing vector
//! names itself in the test output, and a newly vendored set that forgets a
//! vector shows up as a missing test rather than a quietly shorter loop.
//!
//! Split out of `geometry_conformance_test.rs` on 2026-09-01 (slice D); see
//! that file for the contract's provenance and the honest-scope notes.

mod common;

use bloomery_core::geometry::kv_bytes_per_token;
use serde_json::Value;

use common::gguf_vectors::{check_windows, expected_u64, has_ssm_keys, load_vector, parse_vector};

fn banned_u64(vector: &Value, field: &str) -> Vec<u64> {
    vector["must_not_equal"]
        .get(field)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

/// Runs every assertion the vector states. Called by one `#[test]` per vector
/// so a failure names the model it came from.
fn check_vector(id: &str) {
    let vector = load_vector(id);
    let meta = parse_vector(&vector);

    // R2 — KV bytes per token, and R1/R3 through the head_dim and
    // attention-layer counts that feed it.
    if let Some(expected_kv) = expected_u64(&vector, "kv_bytes_per_token") {
        assert_eq!(
            kv_bytes_per_token(&meta),
            expected_kv,
            "R2: kv_bytes_per_token for {id} (head_dim {}, kv_heads {}, attention_layers {})",
            meta.head_dim,
            meta.kv_heads,
            meta.attention_layers
        );
    }
    for banned in banned_u64(&vector, "kv_bytes_per_token") {
        assert_ne!(
            kv_bytes_per_token(&meta),
            banned,
            "R1/R3: {id} reproduced a pinned historical wrong answer ({})",
            vector["must_not_equal"]["note"]
        );
    }

    // R3 — attention layer count comes from full_attention_interval, never the
    // raw block count.
    if let Some(expected_layers) = expected_u64(&vector, "attention_layers") {
        assert_eq!(
            u64::from(meta.attention_layers),
            expected_layers,
            "R3: attention_layers for {id}"
        );
    }

    // R6 — serving block count excludes MTP layers.
    if let Some(expected_blocks) = expected_u64(&vector, "serving_block_count") {
        assert_eq!(
            u64::from(meta.layers),
            expected_blocks,
            "R6: serving_block_count for {id}"
        );
    }
    for banned in banned_u64(&vector, "serving_block_count") {
        assert_ne!(
            u64::from(meta.layers),
            banned,
            "R6: {id} counted an MTP layer as a serving layer ({})",
            vector["must_not_equal"]["note"]
        );
    }

    // R4 — recurrent state is a charged term; zero ONLY without recurrent layers.
    match expected_u64(&vector, "recurrent_state_bytes") {
        Some(expected_bytes) => assert_eq!(
            meta.recurrent_state_bytes, expected_bytes,
            "R4: recurrent_state_bytes for {id}"
        ),
        None => assert!(
            !has_ssm_keys(&vector),
            "R4: {id} carries ssm.* keys but the vector states no expected \
             recurrent_state_bytes — the test would be asserting nothing"
        ),
    }
    if !has_ssm_keys(&vector) {
        assert_eq!(
            meta.recurrent_state_bytes, 0,
            "R4: {id} has no ssm.* keys, so zero is the derived value, not a default"
        );
    }

    // R7 — the window law, on the terms the vector states.
    check_windows(id, &vector, &meta);
}

// ---------------------------------------------------------------------------
// one #[test] per vector
// ---------------------------------------------------------------------------

#[test]
fn codegemma_7b_instruct_q8_0() {
    check_vector("codegemma-7b-instruct-q8_0");
}

/// The v3 headline: the MLA case R9 was written for. `metadata` states
/// `key_length` 192 and `value_length` 128 — unchanged since v2 — and
/// `check_vector` writes both into the synthetic GGUF header and drives them
/// through `parse_gguf_meta` like every other vector, so `kv_bytes_per_token`
/// takes R9's K+V-sum branch (`value_length != head_dim`) rather than the
/// dense "2x head_dim" formula. `expected.kv_bytes_per_token` is now 276,480
/// (the measured allocation) instead of the v1/v2 pin of 331,776 (R2
/// arithmetic on `key_length` alone, applied to both K and V, never
/// observed); see the module doc's "What v3 changes" section for the full
/// story. `must_not_equal` bans that old pin alongside the pre-R1 formula and
/// two disproved MLA-latent guesses — all four checked against bloomery's
/// own computed value via `check_vector`'s generic `banned_u64` loop, the
/// same ban-held-via-real-computation pattern the mtp-trap and qwen3.8-27b
/// vectors already use above.
#[test]
fn deepseek_coder_v2_16b_lite_instruct_q5_k_m() {
    check_vector("deepseek-coder-v2-16b-lite-instruct-q5_K_M");
}

#[test]
fn gemma2_9b() {
    check_vector("gemma2-9b");
}

#[test]
fn mistral_nemo_latest() {
    check_vector("mistral-nemo-latest");
}

#[test]
fn qwen2_5_coder_1_5b_instruct_q8_0() {
    check_vector("qwen2.5-coder-1.5b-instruct-q8_0");
}

#[test]
fn qwen2_5_coder_14b_instruct_q4_k_m() {
    check_vector("qwen2.5-coder-14b-instruct-q4_K_M");
}

/// Also carries the three R7 window scenarios.
#[test]
fn qwen2_5_coder_7b_instruct_q8_0() {
    check_vector("qwen2.5-coder-7b-instruct-q8_0");
}

/// The hybrid: R3 (10 attention layers of 40 blocks) and R4 (62.81 MiB
/// recurrent state), both hardware-verified across two boots.
#[test]
fn qwen3_6_35b_a3b_reap48_ours_q4km() {
    check_vector("qwen3.6-35b-a3b-reap48-ours-q4km");
}

/// The MTP trap: `block_count 41` + `nextn_predict_layers 1` describing 40
/// blocks of tensors.
///
/// This test was `#[ignore]`d and RED from 3f596ef to 3fbc7b1 — `parse_gguf_meta`
/// read `{arch}.block_count` raw and never looked at
/// `{arch}.nextn_predict_layers`, so R6 was unimplemented in the Rust reader
/// (it was implemented only in `tools/flywheel/prune/prune.py`, which zeroes
/// the key at conversion time — a different layer, and one that covers only
/// artifacts this repo produced). Measured then: `serving_block_count` 41 vs
/// the contract's 40, and `recurrent_state_bytes` 68,059,136 vs 65,863,680 (a
/// 2,195,456 B / 2.09 MiB per-context over-charge — one extra recurrent
/// layer). `kv_bytes_per_token` 20,480 and `attention_layers` 10 conformed
/// even in the trap state (41/4 == 10), so the divergence was confined to the
/// block count and the term derived from it.
///
/// `gguf.rs::resolve_serving_block_count` now implements R6, so the assertion
/// runs in normal CI, unchanged from the form it was written in.
#[test]
fn qwen3_6_35b_a3b_reap48_mtp_trap() {
    check_vector("qwen3.6-35b-a3b-reap48-mtp-trap");
}

/// New in v2, and the reason the re-vendor is worth doing: one model that
/// exercises R3, R4 and R6 *together*, hardware-verified. `qwen35`,
/// `block_count` 65 with `nextn_predict_layers` 1 (R6 -> 64 serving),
/// `full_attention_interval` 4 (R3 -> 16 attention layers of those 64), and a
/// full `ssm.*` block over the remaining 48 recurrent layers (R4 ->
/// 156,893,184 B/ctx). The vector's banned answers are assay's own published
/// figures, not hypotheticals: 266,240 B/token is all 65 raw blocks charged as
/// attention layers (a 4.0625x over-charge, upstream erratum E2), and 65 is
/// that raw block count.
///
/// This is the case v1 deliberately withheld rather than pin at the
/// over-charged value; it entered v2 on the live conforming run that closed the
/// erratum. `check_vector` drives it through `parse_gguf_meta` like every other
/// vector, so all four terms are bloomery's own derivations.
#[test]
fn qwen3_8_27b() {
    check_vector("qwen3.8-27b");
}

/// The v2 headline conformance, pinned as literals rather than read from the
/// vector's `expected` block — the same belt-and-braces shape as
/// `qwen3_6_35b_a3b_reap48_mtp_trap_r6_conformance_is_pinned`, so a regression
/// in `parse_gguf_meta` is caught here even if the vendored vector ever moves.
/// The chain is R6 -> R3 -> R2 and R6 -> R4: every number below is downstream
/// of the serving-block subtraction, which is why the raw-block answer is
/// banned at both ends.
#[test]
fn qwen3_8_27b_r3_r4_r6_conformance_is_pinned() {
    let vector = load_vector("qwen3.8-27b");
    let meta = parse_vector(&vector);

    // R6: 65 raw blocks, one of them an MTP layer that never serves a token.
    assert_eq!(
        u64::from(meta.layers),
        64,
        "R6: the MTP layer is not a serving layer (65 - 1)"
    );
    assert_ne!(
        u64::from(meta.layers),
        banned_u64(&vector, "serving_block_count")[0],
        "the raw block count stays banned"
    );

    // R3: 64 serving / interval 4, never the 65 raw blocks.
    assert_eq!(
        meta.attention_layers, 16,
        "R3: serving 64 / interval 4 == 16"
    );

    // R1/R2: key_length 256 authoritative (embedding 5120 / 24 heads would be
    // 213), 2 * 16 * 4 * 256 * 2.
    assert_eq!(meta.head_dim, 256, "R1: key_length is authoritative");
    assert_eq!(kv_bytes_per_token(&meta), 65_536, "R2 on the v2 hybrid");
    assert_ne!(
        kv_bytes_per_token(&meta),
        266_240,
        "the all-65-blocks answer (4.0625x over-charge) stays banned"
    );

    // R4: the 48 non-attention layers each carry a recurrent state.
    assert_eq!(
        meta.recurrent_state_bytes, 156_893_184,
        "R4: 48 recurrent layers charged, not zero and not all 64"
    );
}

/// The inverse of the tripwire this test used to be. Until R6 landed it pinned
/// bloomery's *divergence* (`serving_block_count` 41, `recurrent_state_bytes`
/// 68,059,136) so the gap could not rot into silence, and was designed to fail
/// the moment the fix arrived — it did. It now pins the *conforming* values on
/// the same vector, spelled out as literals rather than read from the vector's
/// `expected` block, so a regression in `parse_gguf_meta` is caught here even
/// if the vendored vector ever moves. Git history from 3fbc7b1 carries the
/// divergence form.
#[test]
fn qwen3_6_35b_a3b_reap48_mtp_trap_r6_conformance_is_pinned() {
    let vector = load_vector("qwen3.6-35b-a3b-reap48-mtp-trap");
    let meta = parse_vector(&vector);

    // The terms that conformed even in the trap state, and still do.
    assert_eq!(kv_bytes_per_token(&meta), 20_480, "R2/R3 hold on the trap");
    assert_eq!(
        meta.attention_layers, 10,
        "R3: serving 40 / interval 4 == 10"
    );
    assert_ne!(
        kv_bytes_per_token(&meta),
        81_920,
        "the all-blocks answer stays banned"
    );

    // R6, and the term downstream of it — the two that used to diverge.
    assert_eq!(
        u64::from(meta.layers),
        40,
        "R6: the MTP layer is not a serving layer (41 - 1)"
    );
    assert_ne!(
        u64::from(meta.layers),
        banned_u64(&vector, "serving_block_count")[0],
        "the raw block count stays banned"
    );
    assert_eq!(
        meta.recurrent_state_bytes, 65_863_680,
        "30 recurrent layers charged, not 31 — the 2_195_456 B per-context \
         over-charge is gone"
    );
}

/// The v3 headline conformance, pinned as literals rather than read from the
/// vector's `expected` block — the same belt-and-braces shape as
/// `qwen3_8_27b_r3_r4_r6_conformance_is_pinned`, so a regression in R9 (the
/// K+V-sum branch of `kv_bytes_per_token`) is caught here even if the
/// vendored vector ever moves. Until Task 9 (commit `3e326c6`) implemented
/// R9, this same vector's v2 pin of 331,776 held through
/// `check_vector`; R9 landing flipped it RED (331,776 is now a banned
/// candidate, not the expected value) before this re-vendor closed the gap
/// with the measured figure.
#[test]
fn deepseek_coder_v2_16b_lite_instruct_q5_k_m_r9_conformance_is_pinned() {
    let vector = load_vector("deepseek-coder-v2-16b-lite-instruct-q5_K_M");
    let meta = parse_vector(&vector);

    // The stated K/V widths, read verbatim by parse_gguf_meta — the two keys
    // R9 needs to tell an MLA model from a dense one.
    assert_eq!(meta.head_dim, 192, "key_length, read verbatim (R1)");
    assert_eq!(
        meta.value_length,
        Some(128),
        "value_length, read verbatim and distinct from head_dim (R9's trigger)"
    );

    // R9: K+V sum, not the dense 2x head_dim factor. attention_layers 27,
    // kv_heads 16: 27 * 16 * (192 + 128) * 2 == 276,480.
    assert_eq!(
        kv_bytes_per_token(&meta),
        276_480,
        "R9: MLA separate-widths formula on the measured allocation"
    );

    // Every disproved candidate stays banned against the real computation,
    // not just against the vector's own `must_not_equal` list.
    for banned in [221_184_u64, 331_776, 31_104, 62_208] {
        assert_ne!(
            kv_bytes_per_token(&meta),
            banned,
            "disproved MLA/dense candidate {banned} stays banned"
        );
    }
    assert_eq!(
        banned_u64(&vector, "kv_bytes_per_token"),
        vec![221_184, 331_776, 31_104, 62_208],
        "the vector's own must_not_equal list matches what this test pins"
    );
}
