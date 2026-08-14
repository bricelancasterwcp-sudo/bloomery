//! GGUF v3 metadata reader.
//!
//! Reads only the header and key-value metadata section of a GGUF file
//! (magic, version, tensor_count, kv_count, then typed key-value pairs).
//! Tensor data itself is never read; `weights_bytes` is derived from the
//! file's total size on disk.

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

/// A subset of the parsed GGUF metadata needed to compute paging geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GgufMeta {
    pub arch: String,
    pub layers: u32,
    pub kv_heads: u32,
    pub head_dim: u32,
    pub training_ctx: u32,
    pub weights_bytes: u64,
}

/// Errors that can occur while parsing a GGUF file's metadata.
#[derive(Debug)]
pub enum GgufError {
    BadMagic,
    UnsupportedVersion(u32),
    MissingKey(String),
    Io(std::io::Error),
}

impl fmt::Display for GgufError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GgufError::BadMagic => write!(f, "bad GGUF magic"),
            GgufError::UnsupportedVersion(v) => write!(f, "unsupported GGUF version: {v}"),
            GgufError::MissingKey(k) => write!(f, "missing GGUF key: {k}"),
            GgufError::Io(e) => write!(f, "GGUF I/O error: {e}"),
        }
    }
}

impl std::error::Error for GgufError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GgufError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for GgufError {
    fn from(e: std::io::Error) -> Self {
        GgufError::Io(e)
    }
}

/// A value read from the GGUF key-value metadata section.
///
/// `Array` intentionally discards its contents: none of the metadata this
/// parser needs lives inside array values, so elements are read only far
/// enough to skip past them correctly.
#[derive(Debug, Clone, PartialEq)]
enum GgufValue {
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    F32(f32),
    Bool(bool),
    Str(String),
    U64(u64),
    I64(i64),
    F64(f64),
    Array,
}

// GGUF value type tags (see the GGUF spec's `gguf_metadata_value_type` enum).
const GGUF_TYPE_U8: u32 = 0;
const GGUF_TYPE_I8: u32 = 1;
const GGUF_TYPE_U16: u32 = 2;
const GGUF_TYPE_I16: u32 = 3;
const GGUF_TYPE_U32: u32 = 4;
const GGUF_TYPE_I32: u32 = 5;
const GGUF_TYPE_F32: u32 = 6;
const GGUF_TYPE_BOOL: u32 = 7;
const GGUF_TYPE_STRING: u32 = 8;
const GGUF_TYPE_ARRAY: u32 = 9;
const GGUF_TYPE_U64: u32 = 10;
const GGUF_TYPE_I64: u32 = 11;
const GGUF_TYPE_F64: u32 = 12;

fn read_exact_buf<R: Read>(reader: &mut R, n: usize) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_u32<R: Read>(reader: &mut R) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64<R: Read>(reader: &mut R) -> io::Result<u64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

fn read_i8<R: Read>(reader: &mut R) -> io::Result<i8> {
    let mut buf = [0u8; 1];
    reader.read_exact(&mut buf)?;
    Ok(buf[0] as i8)
}

fn read_u8<R: Read>(reader: &mut R) -> io::Result<u8> {
    let mut buf = [0u8; 1];
    reader.read_exact(&mut buf)?;
    Ok(buf[0])
}

fn read_u16<R: Read>(reader: &mut R) -> io::Result<u16> {
    let mut buf = [0u8; 2];
    reader.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

fn read_i16<R: Read>(reader: &mut R) -> io::Result<i16> {
    let mut buf = [0u8; 2];
    reader.read_exact(&mut buf)?;
    Ok(i16::from_le_bytes(buf))
}

fn read_i32<R: Read>(reader: &mut R) -> io::Result<i32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(i32::from_le_bytes(buf))
}

fn read_f32<R: Read>(reader: &mut R) -> io::Result<f32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(f32::from_le_bytes(buf))
}

fn read_i64<R: Read>(reader: &mut R) -> io::Result<i64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf)?;
    Ok(i64::from_le_bytes(buf))
}

fn read_f64<R: Read>(reader: &mut R) -> io::Result<f64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf)?;
    Ok(f64::from_le_bytes(buf))
}

/// Rejects a file-provided length before it is used for an allocation or a
/// loop bound. Every byte a well-formed GGUF file claims a string/array
/// holds (or a kv pair count) must actually fit inside the file, so any
/// declared length greater than the file's own size is corrupt data — never
/// a reason to attempt a multi-terabyte allocation or an unbounded loop.
fn check_len_bound(len: u64, file_len: u64) -> io::Result<()> {
    if len > file_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("declared length {len} exceeds file size {file_len} bytes"),
        ));
    }
    Ok(())
}

fn read_gguf_string<R: Read>(reader: &mut R, file_len: u64) -> io::Result<String> {
    let len = read_u64(reader)?;
    check_len_bound(len, file_len)?;
    let bytes = read_exact_buf(reader, len as usize)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reads one typed value for the given type tag, discarding array payloads
/// element-by-element (strings inside arrays each carry their own length).
fn read_gguf_value<R: Read>(reader: &mut R, type_tag: u32, file_len: u64) -> io::Result<GgufValue> {
    match type_tag {
        GGUF_TYPE_U8 => Ok(GgufValue::U8(read_u8(reader)?)),
        GGUF_TYPE_I8 => Ok(GgufValue::I8(read_i8(reader)?)),
        GGUF_TYPE_U16 => Ok(GgufValue::U16(read_u16(reader)?)),
        GGUF_TYPE_I16 => Ok(GgufValue::I16(read_i16(reader)?)),
        GGUF_TYPE_U32 => Ok(GgufValue::U32(read_u32(reader)?)),
        GGUF_TYPE_I32 => Ok(GgufValue::I32(read_i32(reader)?)),
        GGUF_TYPE_F32 => Ok(GgufValue::F32(read_f32(reader)?)),
        GGUF_TYPE_BOOL => Ok(GgufValue::Bool(read_u8(reader)? != 0)),
        GGUF_TYPE_STRING => Ok(GgufValue::Str(read_gguf_string(reader, file_len)?)),
        GGUF_TYPE_U64 => Ok(GgufValue::U64(read_u64(reader)?)),
        GGUF_TYPE_I64 => Ok(GgufValue::I64(read_i64(reader)?)),
        GGUF_TYPE_F64 => Ok(GgufValue::F64(read_f64(reader)?)),
        GGUF_TYPE_ARRAY => {
            let elem_type = read_u32(reader)?;
            let len = read_u64(reader)?;
            check_len_bound(len, file_len)?;
            for _ in 0..len {
                // Discard: we only need to advance the cursor correctly.
                read_gguf_value(reader, elem_type, file_len)?;
            }
            Ok(GgufValue::Array)
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown GGUF value type tag: {other}"),
        )),
    }
}

/// Reads the GGUF header and key-value metadata section into a map.
fn read_kv_map<R: Read>(
    reader: &mut R,
    kv_count: u64,
    file_len: u64,
) -> io::Result<HashMap<String, GgufValue>> {
    let mut kvs = HashMap::new();
    for _ in 0..kv_count {
        let key = read_gguf_string(reader, file_len)?;
        let type_tag = read_u32(reader)?;
        let value = read_gguf_value(reader, type_tag, file_len)?;
        kvs.insert(key, value);
    }
    Ok(kvs)
}

/// Looks up an integer-valued key, accepting either the `U32` or `U64` tag
/// (real-world GGUF files vary in which width they use for the same field).
fn lookup_int(kvs: &HashMap<String, GgufValue>, key: &str) -> Result<u64, GgufError> {
    match kvs.get(key) {
        Some(GgufValue::U32(v)) => Ok(*v as u64),
        Some(GgufValue::U64(v)) => Ok(*v),
        _ => Err(GgufError::MissingKey(key.to_string())),
    }
}

fn lookup_u32(kvs: &HashMap<String, GgufValue>, key: &str) -> Result<u32, GgufError> {
    lookup_int(kvs, key).map(|v| v as u32)
}

fn lookup_string(kvs: &HashMap<String, GgufValue>, key: &str) -> Result<String, GgufError> {
    match kvs.get(key) {
        Some(GgufValue::Str(s)) => Ok(s.clone()),
        _ => Err(GgufError::MissingKey(key.to_string())),
    }
}

/// Resolves `head_dim`: prefers `{arch}.attention.key_length`, else falls
/// back to `{arch}.embedding_length / {arch}.attention.head_count`. In the
/// fallback branch both keys are required; `MissingKey` names whichever one
/// is absent.
fn resolve_head_dim(kvs: &HashMap<String, GgufValue>, arch: &str) -> Result<u32, GgufError> {
    let key_length_key = format!("{arch}.attention.key_length");
    if let Ok(v) = lookup_u32(kvs, &key_length_key) {
        return Ok(v);
    }

    let embedding_length_key = format!("{arch}.embedding_length");
    let head_count_key = format!("{arch}.attention.head_count");
    let embedding_length = lookup_u32(kvs, &embedding_length_key)?;
    let head_count = lookup_u32(kvs, &head_count_key)?;
    if head_count == 0 {
        return Err(GgufError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{head_count_key} is zero"),
        )));
    }
    Ok(embedding_length / head_count)
}

/// Parses the metadata section of a GGUF v3 file at `path`.
pub fn parse_gguf_meta(path: &Path) -> Result<GgufMeta, GgufError> {
    let file_len = std::fs::metadata(path)?.len();
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    if &magic != b"GGUF" {
        return Err(GgufError::BadMagic);
    }

    let version = read_u32(&mut reader)?;
    if version != 3 {
        return Err(GgufError::UnsupportedVersion(version));
    }

    let _tensor_count = read_u64(&mut reader)?;
    let kv_count = read_u64(&mut reader)?;
    check_len_bound(kv_count, file_len)?;

    let kvs = read_kv_map(&mut reader, kv_count, file_len)?;

    let arch = lookup_string(&kvs, "general.architecture")?;
    let layers = lookup_u32(&kvs, &format!("{arch}.block_count"))?;
    let kv_heads = lookup_u32(&kvs, &format!("{arch}.attention.head_count_kv"))?;
    let head_dim = resolve_head_dim(&kvs, &arch)?;
    let training_ctx = lookup_u32(&kvs, &format!("{arch}.context_length"))?;

    Ok(GgufMeta {
        arch,
        layers,
        kv_heads,
        head_dim,
        training_ctx,
        weights_bytes: file_len,
    })
}
