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
