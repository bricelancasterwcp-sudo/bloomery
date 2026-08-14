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
