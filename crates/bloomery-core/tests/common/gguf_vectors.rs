//! The gguf-geometry vendored-vector harness: sha-pinned set loading, minimal
//! GGUF v3 synthesis from a vector's metadata block, and the conformance
//! checks both `geometry_conformance_*` files run.
//!
//! Split out on 2026-09-01 (carried-debt slice D).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bloomery_core::geometry::{kv_bytes_per_token, usable_window, BoundBy, GeometryInput};
use bloomery_core::gguf::{parse_gguf_meta, GgufMeta};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// sha256 of the vendored `MANIFEST.json`, computed at vendoring time
/// (2026-08-28) from `gguf-geometry/vectors/v3/MANIFEST.json` at master
/// `84f042b`. The manifest in turn pins every vector file, so this single
/// constant pins the whole set: a vendored copy that has drifted from the
/// published contract cannot be read by any test in this file.
///
/// Set history: v1's manifest hashed
/// `44f8af208d4bd79055cdaa9e0b9c4e9fa81f305d5f81ab6e87457de6c3fa470a`; v2's
/// hashed `06da801b5dc57fedbfd42555c377c1cd3b6b8fb3c549cd2d367396447fc15116`.
/// A new set is a visible diff here, which is the point of the pin.
pub const MANIFEST_SHA256: &str =
    "c4d5c22d99e658e21d7197fffe915969a9a1d1fe62683a9c3e7d85b884798e4b";

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Reads the vendored manifest, refusing to proceed unless its own bytes hash
/// to `MANIFEST_SHA256`.
pub fn manifest() -> Value {
    let path = data_dir().join("MANIFEST.json");
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert_eq!(
        sha256_hex(&bytes),
        MANIFEST_SHA256,
        "vendored MANIFEST.json does not match the pinned sha256 — the vendored \
         copy has drifted from gguf-geometry vectors/v3 (re-vendor byte-exact, \
         do not re-pin)"
    );
    serde_json::from_slice(&bytes).expect("MANIFEST.json is valid JSON")
}

/// Loads one vector, verifying its bytes against the (already sha-pinned)
/// manifest first. Every assertion in this file reaches its vector through
/// here, so a tampered vector file fails the test that reads it.
pub fn load_vector(id: &str) -> Value {
    let manifest = manifest();
    let file_name = format!("{id}.json");
    let expected_sha = manifest["files"][&file_name]
        .as_str()
        .unwrap_or_else(|| panic!("MANIFEST.json lists no entry for {file_name}"))
        .to_string();
    let path = data_dir().join(&file_name);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert_eq!(
        sha256_hex(&bytes),
        expected_sha,
        "vendored {file_name} does not match its manifest sha256"
    );
    let v: Value = serde_json::from_slice(&bytes).expect("vector is valid JSON");
    assert_eq!(v["id"], id, "vector file name and its id disagree");
    v
}

pub fn push_key(buf: &mut Vec<u8>, key: &str) {
    buf.extend((key.len() as u64).to_le_bytes());
    buf.extend(key.as_bytes());
}

pub fn kv_string(buf: &mut Vec<u8>, key: &str, val: &str) {
    push_key(buf, key);
    buf.extend(8u32.to_le_bytes()); // GGUF type tag: string
    buf.extend((val.len() as u64).to_le_bytes());
    buf.extend(val.as_bytes());
}

pub fn kv_u32(buf: &mut Vec<u8>, key: &str, val: u32) {
    push_key(buf, key);
    buf.extend(4u32.to_le_bytes()); // GGUF type tag: u32
    buf.extend(val.to_le_bytes());
}

/// Writes the vector's `metadata` block into a minimal GGUF v3 file and hands
/// the path back. `null` values are written as *absent keys*, which is the
/// state the vector is recording. Nothing else is added: the reader sees
/// exactly the keys the contract vector carries.
pub fn write_vector_gguf(vector: &Value) -> PathBuf {
    let metadata = vector["metadata"]
        .as_object()
        .expect("vector carries a metadata object");

    let mut kvs = Vec::new();
    let mut kv_count: u64 = 0;
    for (key, value) in metadata {
        match value {
            Value::Null => continue, // absent key — the vector's own statement
            Value::String(s) => kv_string(&mut kvs, key, s),
            Value::Number(n) => {
                let n = n.as_u64().unwrap_or_else(|| panic!("{key} is not u64"));
                let n = u32::try_from(n).unwrap_or_else(|_| panic!("{key} exceeds u32"));
                kv_u32(&mut kvs, key, n);
            }
            other => panic!("unsupported metadata value for {key}: {other}"),
        }
        kv_count += 1;
    }

    // Cargo runs the tests in this binary on parallel threads and two of them
    // synthesise the same vector, so the file name carries a per-call serial:
    // a shared path would let one thread truncate the file another is reading.
    static SERIAL: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join("bloomery-geometry-conformance-v3");
    fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(format!(
        "{}.{}.gguf",
        vector["id"].as_str().expect("vector has an id"),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let mut f = fs::File::create(&path).expect("create synthetic gguf");
    f.write_all(b"GGUF").unwrap();
    f.write_all(&3u32.to_le_bytes()).unwrap(); // version
    f.write_all(&0u64.to_le_bytes()).unwrap(); // tensor_count
    f.write_all(&kv_count.to_le_bytes()).unwrap();
    f.write_all(&kvs).unwrap();
    f.flush().unwrap();
    path
}

pub fn parse_vector(vector: &Value) -> GgufMeta {
    let path = write_vector_gguf(vector);
    parse_gguf_meta(&path).unwrap_or_else(|e| {
        panic!(
            "bloomery refused vector {}: {e}",
            vector["id"].as_str().unwrap_or("?")
        )
    })
}

pub fn expected_u64(vector: &Value, field: &str) -> Option<u64> {
    vector["expected"].get(field).and_then(Value::as_u64)
}

/// True when the vector's metadata carries no `{arch}.ssm.*` key, i.e. the
/// architecture has no recurrent layers. R4 permits `recurrent_state_bytes ==
/// 0` only in exactly this case.
pub fn has_ssm_keys(vector: &Value) -> bool {
    vector["metadata"]
        .as_object()
        .expect("metadata object")
        .keys()
        .any(|k| k.contains(".ssm."))
}

pub fn bound_by_for(limited_by: &str) -> BoundBy {
    match limited_by {
        "budget" => BoundBy::Vram,
        "training_ctx" => BoundBy::TrainingCtx,
        "user_cap" => BoundBy::UserCap,
        "measured_ceiling" => BoundBy::MeasuredCeiling,
        other => panic!("vector names an unmapped limited_by term: {other}"),
    }
}

/// Maps the contract's named window terms onto `GeometryInput`:
///
/// | contract term          | GeometryInput field |
/// |------------------------|---------------------|
/// | `training_ctx`         | `training_ctx`      |
/// | `budget_bytes`         | `free_vram_bytes`   |
/// | `weights_bytes`        | `weights_bytes`     |
/// | `fixed_overhead_bytes` | `overhead_bytes`    |
/// | `kv_bytes_per_token`   | `kv_per_token`      |
/// | `user_cap`             | `user_cap`          |
///
/// `recurrent_state_bytes` folds into bloomery's `ctx_overhead_bytes`; in v2 as
/// in v1 the only window scenarios in the set are qwen2.5-coder-7b's three,
/// which sit on a dense model and state no recurrent term, so it is 0 here — a
/// stated term, not a dropped one. (`qwen3.8-27b` states no windows at all:
/// upstream records that its erratum's conforming window figures stay
/// condition-parameterized under a box state no later run can re-occupy.)
/// `measured_ceiling` is a
/// bloomery-only extra term, unmeasured (`None`) in every scenario, so per R7
/// it drops out rather than being guessed.
pub fn check_windows(id: &str, vector: &Value, meta: &GgufMeta) {
    let Some(scenarios) = vector["expected"].get("windows").and_then(Value::as_array) else {
        return;
    };
    assert!(!scenarios.is_empty(), "{id}: empty windows array");

    for (n, scenario) in scenarios.iter().enumerate() {
        let terms = &scenario["terms"];
        let kv_per_token = terms["kv_bytes_per_token"]
            .as_u64()
            .expect("scenario states kv_bytes_per_token");
        assert_eq!(
            kv_bytes_per_token(meta),
            kv_per_token,
            "{id} window {n}: the scenario's kv term disagrees with the derived one"
        );

        let input = GeometryInput {
            training_ctx: terms["training_ctx"].as_u64().expect("training_ctx") as u32,
            kv_per_token,
            weights_bytes: terms["weights_bytes"].as_u64().expect("weights_bytes"),
            free_vram_bytes: Some(terms["budget_bytes"].as_u64().expect("budget_bytes")),
            overhead_bytes: terms["fixed_overhead_bytes"]
                .as_u64()
                .expect("fixed_overhead_bytes"),
            ctx_overhead_bytes: 0,
            user_cap: terms["user_cap"].as_u64().map(|c| c as u32),
            measured_ceiling: None,
        };
        let window = usable_window(&input);

        let expected_tokens = scenario["usable_window"].as_u64().expect("usable_window") as u32;
        let expected_bound = bound_by_for(scenario["limited_by"].as_str().expect("limited_by"));
        assert_eq!(
            window.tokens, expected_tokens,
            "R7: {id} window {n} usable_window ({})",
            scenario["note"]
        );
        assert_eq!(
            window.bound_by, expected_bound,
            "R7: {id} window {n} limited_by"
        );
        assert!(
            !window.vram_unmeasured,
            "R7: {id} window {n} states a budget, so the VRAM term was measured"
        );
    }
}

pub fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/gguf_geometry_v3")
}
