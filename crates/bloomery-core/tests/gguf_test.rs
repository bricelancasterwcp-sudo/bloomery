use std::io::Write;

use bloomery_core::gguf::{parse_gguf_meta, GgufError};

fn kv_string(buf: &mut Vec<u8>, key: &str, val: &str) {
    buf.extend((key.len() as u64).to_le_bytes());
    buf.extend(key.as_bytes());
    buf.extend(8u32.to_le_bytes());
    buf.extend((val.len() as u64).to_le_bytes());
    buf.extend(val.as_bytes());
}
fn kv_u32(buf: &mut Vec<u8>, key: &str, val: u32) {
    buf.extend((key.len() as u64).to_le_bytes());
    buf.extend(key.as_bytes());
    buf.extend(4u32.to_le_bytes());
    buf.extend(val.to_le_bytes());
}

/// Array-of-strings KV (type tag 9, elem_type 8): each element carries its
/// own u64 length prefix, same as a top-level string value.
fn kv_array_of_strings(buf: &mut Vec<u8>, key: &str, vals: &[&str]) {
    buf.extend((key.len() as u64).to_le_bytes());
    buf.extend(key.as_bytes());
    buf.extend(9u32.to_le_bytes()); // type tag: array
    buf.extend(8u32.to_le_bytes()); // elem type: string
    buf.extend((vals.len() as u64).to_le_bytes());
    for v in vals {
        buf.extend((v.len() as u64).to_le_bytes());
        buf.extend(v.as_bytes());
    }
}

/// Array-of-u32 KV (type tag 9, elem_type 4).
fn kv_array_of_u32(buf: &mut Vec<u8>, key: &str, vals: &[u32]) {
    buf.extend((key.len() as u64).to_le_bytes());
    buf.extend(key.as_bytes());
    buf.extend(9u32.to_le_bytes()); // type tag: array
    buf.extend(4u32.to_le_bytes()); // elem type: u32
    buf.extend((vals.len() as u64).to_le_bytes());
    for v in vals {
        buf.extend(v.to_le_bytes());
    }
}

fn write_gguf(path: &std::path::Path, kv_count: u64, kvs: &[u8]) {
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"GGUF").unwrap();
    f.write_all(&3u32.to_le_bytes()).unwrap();
    f.write_all(&0u64.to_le_bytes()).unwrap(); // tensor_count
    f.write_all(&kv_count.to_le_bytes()).unwrap();
    f.write_all(kvs).unwrap();
}

fn write_qwen_like_gguf(path: &std::path::Path) {
    let mut kvs = Vec::new();
    kv_string(&mut kvs, "general.architecture", "qwen2");
    kv_u32(&mut kvs, "qwen2.block_count", 28);
    kv_u32(&mut kvs, "qwen2.attention.head_count_kv", 4);
    kv_u32(&mut kvs, "qwen2.attention.key_length", 128);
    kv_u32(&mut kvs, "qwen2.context_length", 32768);
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"GGUF").unwrap();
    f.write_all(&3u32.to_le_bytes()).unwrap();
    f.write_all(&0u64.to_le_bytes()).unwrap(); // tensor_count
    f.write_all(&5u64.to_le_bytes()).unwrap(); // kv_count
    f.write_all(&kvs).unwrap();
}

#[test]
fn parses_qwen_like_metadata() {
    let dir = std::env::temp_dir().join("bloomery-gguf-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("qwen.gguf");
    write_qwen_like_gguf(&path);
    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!(m.arch, "qwen2");
    assert_eq!(
        (m.layers, m.kv_heads, m.head_dim, m.training_ctx),
        (28, 4, 128, 32768)
    );
    assert_eq!(m.weights_bytes, std::fs::metadata(&path).unwrap().len());
}

#[test]
fn rejects_bad_magic() {
    let dir = std::env::temp_dir().join("bloomery-gguf-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad.gguf");
    std::fs::write(&path, b"NOPE").unwrap();
    assert!(matches!(parse_gguf_meta(&path), Err(GgufError::BadMagic)));
}

#[test]
fn missing_key_is_named() {
    // fixture with architecture but no block_count
    let dir = std::env::temp_dir().join("bloomery-gguf-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("partial.gguf");
    let mut kvs = Vec::new();
    kv_string(&mut kvs, "general.architecture", "qwen2");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(b"GGUF").unwrap();
    f.write_all(&3u32.to_le_bytes()).unwrap();
    f.write_all(&0u64.to_le_bytes()).unwrap();
    f.write_all(&1u64.to_le_bytes()).unwrap();
    f.write_all(&kvs).unwrap();
    match parse_gguf_meta(&path) {
        Err(GgufError::MissingKey(k)) => assert_eq!(k, "qwen2.block_count"),
        other => panic!("expected MissingKey, got {other:?}"),
    }
}

#[test]
fn skips_arrays_before_required_keys() {
    // A string array and a numeric array both appear before the required
    // scalar keys. If array-skipping consumes the wrong number of bytes,
    // every key read after the arrays comes out misaligned/garbage.
    let dir = std::env::temp_dir().join("bloomery-gguf-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("array_skip.gguf");
    let mut kvs = Vec::new();
    kv_array_of_strings(
        &mut kvs,
        "tokenizer.ggml.tokens",
        &["<bos>", "<eos>", "<pad>"],
    );
    kv_array_of_u32(&mut kvs, "qwen2.rope.dimension_sections", &[16, 24, 24, 64]);
    kv_string(&mut kvs, "general.architecture", "qwen2");
    kv_u32(&mut kvs, "qwen2.block_count", 28);
    kv_u32(&mut kvs, "qwen2.attention.head_count_kv", 4);
    kv_u32(&mut kvs, "qwen2.attention.key_length", 128);
    kv_u32(&mut kvs, "qwen2.context_length", 32768);
    write_gguf(&path, 7, &kvs);

    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!(m.arch, "qwen2");
    assert_eq!(
        (m.layers, m.kv_heads, m.head_dim, m.training_ctx),
        (28, 4, 128, 32768)
    );
}

#[test]
fn head_dim_falls_back_to_embedding_over_head_count() {
    let dir = std::env::temp_dir().join("bloomery-gguf-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("head_dim_fallback.gguf");
    let mut kvs = Vec::new();
    kv_string(&mut kvs, "general.architecture", "qwen2");
    kv_u32(&mut kvs, "qwen2.block_count", 28);
    kv_u32(&mut kvs, "qwen2.attention.head_count_kv", 4);
    // no qwen2.attention.key_length: must fall back
    kv_u32(&mut kvs, "qwen2.embedding_length", 3584);
    kv_u32(&mut kvs, "qwen2.attention.head_count", 28);
    kv_u32(&mut kvs, "qwen2.context_length", 32768);
    write_gguf(&path, 6, &kvs);

    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!(m.head_dim, 128);
}

#[test]
fn head_dim_fallback_missing_head_count_is_named() {
    let dir = std::env::temp_dir().join("bloomery-gguf-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("head_dim_fallback_missing_head_count.gguf");
    let mut kvs = Vec::new();
    kv_string(&mut kvs, "general.architecture", "qwen2");
    kv_u32(&mut kvs, "qwen2.block_count", 28);
    kv_u32(&mut kvs, "qwen2.attention.head_count_kv", 4);
    kv_u32(&mut kvs, "qwen2.embedding_length", 3584);
    // no qwen2.attention.head_count
    kv_u32(&mut kvs, "qwen2.context_length", 32768);
    write_gguf(&path, 5, &kvs);

    match parse_gguf_meta(&path) {
        Err(GgufError::MissingKey(k)) => assert_eq!(k, "qwen2.attention.head_count"),
        other => panic!("expected MissingKey, got {other:?}"),
    }
}

#[test]
fn head_dim_fallback_zero_head_count_is_clean_error() {
    let dir = std::env::temp_dir().join("bloomery-gguf-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("head_dim_fallback_zero_head_count.gguf");
    let mut kvs = Vec::new();
    kv_string(&mut kvs, "general.architecture", "qwen2");
    kv_u32(&mut kvs, "qwen2.block_count", 28);
    kv_u32(&mut kvs, "qwen2.attention.head_count_kv", 4);
    kv_u32(&mut kvs, "qwen2.embedding_length", 3584);
    kv_u32(&mut kvs, "qwen2.attention.head_count", 0);
    kv_u32(&mut kvs, "qwen2.context_length", 32768);
    write_gguf(&path, 6, &kvs);

    match parse_gguf_meta(&path) {
        Err(GgufError::Io(_)) => {}
        other => panic!("expected Io error (not a panic), got {other:?}"),
    }
}

#[test]
fn rejects_string_length_exceeding_file_size() {
    // A single KV whose declared string length (2^40) vastly exceeds the
    // actual (tiny) file size. Must fail cleanly, never attempt to
    // allocate ~1TB and never panic/abort the process.
    let dir = std::env::temp_dir().join("bloomery-gguf-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("huge_length.gguf");
    let mut kvs = Vec::new();
    kvs.extend(1u64.to_le_bytes()); // key length = 1
    kvs.extend(b"x");
    kvs.extend(8u32.to_le_bytes()); // type tag: string
    kvs.extend((1u64 << 40).to_le_bytes()); // declared value length: ~1TB
                                            // (no actual string bytes follow — file stays small)
    write_gguf(&path, 1, &kvs);

    match parse_gguf_meta(&path) {
        Err(GgufError::Io(_)) => {}
        other => panic!("expected Io error (not a panic/abort), got {other:?}"),
    }
}

fn write_qwen35moe_like_gguf(
    path: &std::path::Path,
    full_attention_interval: Option<u32>,
    ssm: bool,
) {
    let mut kvs = Vec::new();
    let mut n = 0u64;
    kv_string(&mut kvs, "general.architecture", "qwen35moe");
    n += 1;
    kv_u32(&mut kvs, "qwen35moe.block_count", 40);
    n += 1;
    kv_u32(&mut kvs, "qwen35moe.attention.head_count_kv", 2);
    n += 1;
    kv_u32(&mut kvs, "qwen35moe.attention.key_length", 256);
    n += 1;
    kv_u32(&mut kvs, "qwen35moe.context_length", 262144);
    n += 1;
    if let Some(k) = full_attention_interval {
        kv_u32(&mut kvs, "qwen35moe.full_attention_interval", k);
        n += 1;
    }
    if ssm {
        kv_u32(&mut kvs, "qwen35moe.ssm.conv_kernel", 4);
        n += 1;
        kv_u32(&mut kvs, "qwen35moe.ssm.state_size", 128);
        n += 1;
        kv_u32(&mut kvs, "qwen35moe.ssm.group_count", 16);
        n += 1;
        kv_u32(&mut kvs, "qwen35moe.ssm.inner_size", 4096);
        n += 1;
    }
    write_gguf(path, n, &kvs);
}

#[test]
fn hybrid_meta_counts_attention_layers_and_derives_recurrent_state() {
    let dir = std::env::temp_dir().join("bloomery-gguf-hybrid");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hybrid.gguf");
    write_qwen35moe_like_gguf(&path, Some(4), true);
    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!(m.layers, 40);
    assert_eq!(m.attention_layers, 10, "40 blocks / interval 4");
    // 30 recurrent layers x [(4-1)*(4096 + 2*16*128) + 128*4096] x 4 bytes
    assert_eq!(m.recurrent_state_bytes, 65_863_680);
}

#[test]
fn dense_meta_keeps_attention_layers_equal_to_layers_and_zero_recurrent() {
    let dir = std::env::temp_dir().join("bloomery-gguf-dense2");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("dense.gguf");
    write_qwen_like_gguf(&path);
    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!(m.attention_layers, m.layers);
    assert_eq!(m.recurrent_state_bytes, 0);
}

#[test]
fn interval_without_ssm_keys_still_counts_attention_layers_and_charges_no_state() {
    let dir = std::env::temp_dir().join("bloomery-gguf-hybrid-nossm");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("h.gguf");
    write_qwen35moe_like_gguf(&path, Some(4), false);
    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!((m.attention_layers, m.recurrent_state_bytes), (10, 0));
}

#[test]
fn zero_full_attention_interval_is_invalid_data() {
    let dir = std::env::temp_dir().join("bloomery-gguf-hybrid-zero");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("z.gguf");
    write_qwen35moe_like_gguf(&path, Some(0), true);
    match parse_gguf_meta(&path) {
        Err(GgufError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::InvalidData),
        other => panic!("expected InvalidData, got {other:?}"),
    }
}

/// An interval larger than block_count means no layer ever satisfies
/// llama.cpp's `(i+1) % k == 0` rule — `layers / k` floors to 0. Zero would
/// silently route `kv_bytes_per_token` to the unbounded-window path
/// (geometry.rs), so this must be a clean `InvalidData` error instead.
#[test]
fn full_attention_interval_exceeding_block_count_is_invalid_data() {
    let dir = std::env::temp_dir().join("bloomery-gguf-hybrid-interval-too-big");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("interval_too_big.gguf");
    let mut kvs = Vec::new();
    kv_string(&mut kvs, "general.architecture", "qwen35moe");
    kv_u32(&mut kvs, "qwen35moe.block_count", 3);
    kv_u32(&mut kvs, "qwen35moe.attention.head_count_kv", 2);
    kv_u32(&mut kvs, "qwen35moe.attention.key_length", 256);
    kv_u32(&mut kvs, "qwen35moe.context_length", 262144);
    kv_u32(&mut kvs, "qwen35moe.full_attention_interval", 4);
    write_gguf(&path, 6, &kvs);

    match parse_gguf_meta(&path) {
        Err(GgufError::Io(e)) => {
            assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
            assert!(
                e.to_string()
                    .contains("full_attention_interval exceeds block_count"),
                "expected the exceeds-block_count message, got {e}"
            );
        }
        other => panic!("expected InvalidData, got {other:?}"),
    }
}

/// A non-divisible interval floors per llama.cpp's `(i+1) % k == 0` rule:
/// block_count 42 / interval 4 has layer indices (1-based) 4, 8, ..., 40
/// satisfy the rule — 10 layers, not 10.5 rounded up.
#[test]
fn non_divisible_full_attention_interval_floors() {
    let dir = std::env::temp_dir().join("bloomery-gguf-hybrid-nondivisible");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("nondiv.gguf");
    let mut kvs = Vec::new();
    kv_string(&mut kvs, "general.architecture", "qwen35moe");
    kv_u32(&mut kvs, "qwen35moe.block_count", 42);
    kv_u32(&mut kvs, "qwen35moe.attention.head_count_kv", 2);
    kv_u32(&mut kvs, "qwen35moe.attention.key_length", 256);
    kv_u32(&mut kvs, "qwen35moe.context_length", 262144);
    kv_u32(&mut kvs, "qwen35moe.full_attention_interval", 4);
    write_gguf(&path, 6, &kvs);

    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!(m.attention_layers, 10, "floor(42/4)");
}

/// Controller ruling: a PARTIAL `{arch}.ssm.*` key set (e.g. a Mamba-1 GGUF
/// missing `ssm.group_count`) must still parse cleanly and yields
/// `recurrent_state_bytes == 0` — a partial set is not modeled, so the KV
/// over-count on such a model stays conservative rather than guessing.
#[test]
fn partial_ssm_keys_yield_zero_recurrent_state_bytes() {
    let dir = std::env::temp_dir().join("bloomery-gguf-hybrid-partial-ssm");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("partial_ssm.gguf");
    let mut kvs = Vec::new();
    kv_string(&mut kvs, "general.architecture", "qwen35moe");
    kv_u32(&mut kvs, "qwen35moe.block_count", 40);
    kv_u32(&mut kvs, "qwen35moe.attention.head_count_kv", 2);
    kv_u32(&mut kvs, "qwen35moe.attention.key_length", 256);
    kv_u32(&mut kvs, "qwen35moe.context_length", 262144);
    kv_u32(&mut kvs, "qwen35moe.full_attention_interval", 4);
    kv_u32(&mut kvs, "qwen35moe.ssm.conv_kernel", 4);
    kv_u32(&mut kvs, "qwen35moe.ssm.state_size", 128);
    kv_u32(&mut kvs, "qwen35moe.ssm.inner_size", 4096);
    // qwen35moe.ssm.group_count intentionally absent (Mamba-1-style partial set).
    write_gguf(&path, 9, &kvs);

    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!((m.attention_layers, m.recurrent_state_bytes), (10, 0));
}

// ---------------------------------------------------------------------------
// R6 — serving block count (gguf-geometry SPEC.md; the rule text is unchanged
// across vector sets v1 and v2, and bloomery vendors v2)
//
// `serving_block_count = block_count - {arch}.nextn_predict_layers` when the
// MTP key is present and nonzero. Origin: the REAP-48 prune left
// `mtp_num_hidden_layers: 1` in the HF config, so `convert_hf_to_gguf` sized
// `block_count = 40 + 1 = 41` and emitted `qwen35moe.nextn_predict_layers = 1`
// while writing only 40 blocks of tensors (bloomery
// docs/superpowers/evidence/2026-08-22-reap48-ours-prune-and-acceptance.md,
// BUG #3). R3 and R4 both consume the serving count, so the subtraction has to
// happen before either is derived.
// ---------------------------------------------------------------------------

/// A qwen35moe-like image in the MTP shape: `block_count` blocks of which
/// `nextn` are multi-token-prediction layers (the key is omitted entirely when
/// `nextn` is `None`), plus `full_attention_interval` and the four `ssm.*`
/// keys — so `attention_layers` (R3) and `recurrent_state_bytes` (R4) are both
/// derived from whatever serving count R6 produces, not asserted in isolation.
fn write_mtp_gguf(path: &std::path::Path, block_count: u32, nextn: Option<u32>) {
    let mut kvs = Vec::new();
    let mut n = 0u64;
    kv_string(&mut kvs, "general.architecture", "qwen35moe");
    n += 1;
    kv_u32(&mut kvs, "qwen35moe.block_count", block_count);
    n += 1;
    kv_u32(&mut kvs, "qwen35moe.attention.head_count_kv", 2);
    n += 1;
    kv_u32(&mut kvs, "qwen35moe.attention.key_length", 256);
    n += 1;
    kv_u32(&mut kvs, "qwen35moe.context_length", 262144);
    n += 1;
    kv_u32(&mut kvs, "qwen35moe.full_attention_interval", 4);
    n += 1;
    if let Some(v) = nextn {
        kv_u32(&mut kvs, "qwen35moe.nextn_predict_layers", v);
        n += 1;
    }
    for (k, v) in [
        ("qwen35moe.ssm.conv_kernel", 4u32),
        ("qwen35moe.ssm.state_size", 128),
        ("qwen35moe.ssm.group_count", 16),
        ("qwen35moe.ssm.inner_size", 4096),
    ] {
        kv_u32(&mut kvs, k, v);
        n += 1;
    }
    write_gguf(path, n, &kvs);
}

fn mtp_fixture(name: &str, block_count: u32, nextn: Option<u32>) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("bloomery-gguf-r6");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.gguf"));
    write_mtp_gguf(&path, block_count, nextn);
    path
}

/// The trap itself: 41 declared blocks, 1 of them MTP, 40 actually serving.
/// Every downstream term follows the serving 40, not the declared 41 — 10
/// attention layers (R3) and 30 recurrent layers' worth of state (R4).
#[test]
fn mtp_layers_are_subtracted_from_the_serving_block_count() {
    let path = mtp_fixture("trap", 41, Some(1));
    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!(m.layers, 40, "R6: 41 declared blocks - 1 MTP layer");
    assert_eq!(
        m.attention_layers, 10,
        "R3 consumes the serving count: 40/4"
    );
    // 30 recurrent layers x [(4-1)*(4096 + 2*16*128) + 128*4096] x 4 bytes.
    // Charging the MTP layer as recurrent too would give 31 layers = 68_059_136.
    assert_eq!(
        m.recurrent_state_bytes, 65_863_680,
        "R4 consumes the serving count: 40 - 10 = 30 recurrent layers"
    );
}

/// `nextn_predict_layers: 0` is the state the prune tool patches a trapped
/// artifact into. Zero MTP layers means zero subtraction.
#[test]
fn zero_nextn_predict_layers_leaves_the_block_count_unchanged() {
    let path = mtp_fixture("zero_nextn", 40, Some(0));
    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!(m.layers, 40);
    assert_eq!(m.attention_layers, 10);
    assert_eq!(m.recurrent_state_bytes, 65_863_680);
}

/// The key absent entirely — every model on the box today. R6 must be a no-op
/// here, so this pins that the fix cannot move a model currently in use.
#[test]
fn absent_nextn_predict_layers_leaves_the_block_count_unchanged() {
    let absent = parse_gguf_meta(&mtp_fixture("absent_nextn", 40, None)).unwrap();
    assert_eq!(absent.layers, 40);
    assert_eq!(absent.attention_layers, 10);
    assert_eq!(absent.recurrent_state_bytes, 65_863_680);

    // Absent and 0 are the same geometry; only the file length differs.
    let zero = parse_gguf_meta(&mtp_fixture("absent_nextn_cmp", 40, Some(0))).unwrap();
    assert_eq!(
        (
            absent.layers,
            absent.attention_layers,
            absent.recurrent_state_bytes
        ),
        (
            zero.layers,
            zero.attention_layers,
            zero.recurrent_state_bytes
        ),
    );
}

/// More MTP layers than blocks is not a geometry, it is a corrupt file. R8:
/// refuse and name the key rather than wrapping the subtraction around.
#[test]
fn nextn_predict_layers_exceeding_block_count_is_invalid_data() {
    let path = mtp_fixture("nextn_too_big", 40, Some(41));
    match parse_gguf_meta(&path) {
        Err(GgufError::Io(e)) => {
            assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
            assert!(
                e.to_string().contains("nextn_predict_layers"),
                "the error must name the key it refused on, got {e}"
            );
        }
        other => panic!("expected InvalidData, got {other:?}"),
    }
}

/// All blocks MTP leaves zero serving layers. That is not a model, and zero
/// would route `kv_bytes_per_token` to geometry.rs's unbounded-window path —
/// the same silent "this model has no context limit" the
/// `full_attention_interval` guard above exists to prevent. Refuse instead.
#[test]
fn nextn_predict_layers_equal_to_block_count_is_invalid_data() {
    let path = mtp_fixture("nextn_all_blocks", 40, Some(40));
    match parse_gguf_meta(&path) {
        Err(GgufError::Io(e)) => {
            assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
            assert!(
                e.to_string().contains("nextn_predict_layers"),
                "the error must name the key it refused on, got {e}"
            );
        }
        other => panic!("expected InvalidData, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// R9 — MLA, separate K/V widths (gguf-geometry SPEC.md). Measured branch H-b:
// bloomery reads the stated `{arch}.attention.value_length` verbatim; there
// is no latent-dim refusal at this branch (that is H-c/c'-only, where the
// price cannot be read off the stated widths alone).
// ---------------------------------------------------------------------------

/// A deepseek2-shaped image: `block_count` 27, `head_count_kv` 16,
/// `key_length` 192, and — when `value_length` is `Some` — a stated V width
/// distinct from K's. `value_length` is omitted entirely (not written as 0)
/// when `None`, matching the MTP fixture's "key absent" convention above.
fn write_deepseek2_like_gguf(path: &std::path::Path, value_length: Option<u32>) {
    let mut kvs = Vec::new();
    let mut n = 0u64;
    kv_string(&mut kvs, "general.architecture", "deepseek2");
    n += 1;
    kv_u32(&mut kvs, "deepseek2.block_count", 27);
    n += 1;
    kv_u32(&mut kvs, "deepseek2.attention.head_count_kv", 16);
    n += 1;
    kv_u32(&mut kvs, "deepseek2.attention.key_length", 192);
    n += 1;
    kv_u32(&mut kvs, "deepseek2.context_length", 163840);
    n += 1;
    if let Some(v) = value_length {
        kv_u32(&mut kvs, "deepseek2.attention.value_length", v);
        n += 1;
    }
    write_gguf(path, n, &kvs);
}

fn r9_fixture(name: &str, value_length: Option<u32>) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("bloomery-gguf-r9");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.gguf"));
    write_deepseek2_like_gguf(&path, value_length);
    path
}

/// The stated V width parses straight through onto `GgufMeta.value_length`.
/// key_length (192) still becomes `head_dim`; value_length (128) is a
/// distinct field the parser does not fold into head_dim in any way.
#[test]
fn deepseek2_shaped_metadata_parses_value_length() {
    let m = parse_gguf_meta(&r9_fixture("deepseek2", Some(128))).unwrap();
    assert_eq!(m.arch, "deepseek2");
    assert_eq!(
        (m.layers, m.kv_heads, m.head_dim, m.value_length),
        (27, 16, 192, Some(128))
    );
}

/// A pre-R9 file, or any file that simply never states V's width, must parse
/// with `value_length: None` — never `Some(0)` and never defaulted to
/// `head_dim`. `None` is what downstream (geometry.rs) reads as "dense
/// identity, use the pre-R9 formula unchanged."
#[test]
fn absent_value_length_parses_as_none() {
    let m = parse_gguf_meta(&r9_fixture("no_value_length", None)).unwrap();
    assert_eq!(m.value_length, None);
    // The rest of the dense-model reading is untouched by the new field.
    assert_eq!((m.layers, m.kv_heads, m.head_dim), (27, 16, 192));
}
