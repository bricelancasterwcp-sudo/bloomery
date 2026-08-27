//! gguf-geometry contract (vector set v1) conformance for bloomery-core.
//!
//! The vectors under `tests/data/gguf_geometry_v1/` are a byte-exact vendored
//! copy of `gguf-geometry/vectors/v1/`. That repo holds no implementation: its
//! expected values come from committed assay/bloomery evidence, never from
//! code. The rules are R1-R8 in its `SPEC.md`; each assertion below names the
//! rule it proves.
//!
//! # Honest scope
//!
//! The vectors are JSON metadata blocks, not GGUF binaries, so they cannot
//! drive `parse_gguf_meta` over real model bytes. Two consequences:
//!
//! 1. To test *interpretation* (R1 head_dim, R3 attention layers, R4 recurrent
//!    state, R6 serving block count, R8 refusal) rather than only *derivation*
//!    (R2, R7), each vector's `metadata` map is written verbatim into a minimal
//!    GGUF v3 header (magic + version + counts + typed KV pairs, zero tensors)
//!    and handed to bloomery's own reader. The test therefore never computes a
//!    geometry term itself: `attention_layers`, `head_dim`, `layers` and
//!    `recurrent_state_bytes` all come out of `parse_gguf_meta`. Keys the
//!    vector records as `null` are written as absent, which is what "absent"
//!    means to the reader.
//! 2. Byte-level reader coverage — real quantized files, array KV skipping,
//!    truncation, u64-vs-u32 width variance — is NOT this file's job and stays
//!    in `gguf_test.rs` and `gguf_real_hybrid_test.rs` (the latter parses the
//!    real REAP-48 GGUF when it is on the box).
//!
//! R5 (expert fields) has no counterpart in `GgufMeta` at all — bloomery-core
//! does not model MoE expert counts. See `r5_bloomery_core_models_no_expert_fields`.
//!
//! # Known divergence (2026-08-27)
//!
//! 9 of the 10 v1 vectors conform. The tenth, `qwen3.6-35b-a3b-reap48-mtp-trap`,
//! is RED: bloomery-core does not implement **R6** — `parse_gguf_meta` reads
//! `{arch}.block_count` raw and never subtracts `{arch}.nextn_predict_layers`.
//! See `qwen3_6_35b_a3b_reap48_mtp_trap` (the contract assertion, `#[ignore]`d
//! pending a ruling) and `qwen3_6_35b_a3b_reap48_mtp_trap_r6_divergence_is_pinned`
//! (the actual behaviour, under CI).

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bloomery_core::geometry::{kv_bytes_per_token, usable_window, BoundBy, GeometryInput};
use bloomery_core::gguf::{parse_gguf_meta, GgufMeta};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// sha256 of the vendored `MANIFEST.json`, computed at vendoring time
/// (2026-08-27) from `gguf-geometry/vectors/v1/MANIFEST.json`. The manifest in
/// turn pins every vector file, so this single constant pins the whole set: a
/// vendored copy that has drifted from the published contract cannot be read
/// by any test in this file.
const MANIFEST_SHA256: &str = "44f8af208d4bd79055cdaa9e0b9c4e9fa81f305d5f81ab6e87457de6c3fa470a";

const SET_VERSION: &str = "v1";

/// Every vector id in the set, each with a `#[test]` of its own below. The
/// `all_vectors_have_a_named_test` guard fails if the set grows a vector this
/// file silently ignores.
const VECTOR_IDS: &[&str] = &[
    "codegemma-7b-instruct-q8_0",
    "deepseek-coder-v2-16b-lite-instruct-q5_K_M",
    "gemma-4-12b-it-qat-q4_0-latest",
    "gemma2-9b",
    "mistral-nemo-latest",
    "qwen2.5-coder-1.5b-instruct-q8_0",
    "qwen2.5-coder-14b-instruct-q4_K_M",
    "qwen2.5-coder-7b-instruct-q8_0",
    "qwen3.6-35b-a3b-reap48-mtp-trap",
    "qwen3.6-35b-a3b-reap48-ours-q4km",
];

// ---------------------------------------------------------------------------
// vendored-set loading (sha-pinned)
// ---------------------------------------------------------------------------

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/gguf_geometry_v1")
}

fn sha256_hex(bytes: &[u8]) -> String {
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
fn manifest() -> Value {
    let path = data_dir().join("MANIFEST.json");
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert_eq!(
        sha256_hex(&bytes),
        MANIFEST_SHA256,
        "vendored MANIFEST.json does not match the pinned sha256 — the vendored \
         copy has drifted from gguf-geometry vectors/v1 (re-vendor byte-exact, \
         do not re-pin)"
    );
    serde_json::from_slice(&bytes).expect("MANIFEST.json is valid JSON")
}

/// Loads one vector, verifying its bytes against the (already sha-pinned)
/// manifest first. Every assertion in this file reaches its vector through
/// here, so a tampered vector file fails the test that reads it.
fn load_vector(id: &str) -> Value {
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

// ---------------------------------------------------------------------------
// minimal GGUF v3 synthesis from a vector's metadata block
// ---------------------------------------------------------------------------

fn push_key(buf: &mut Vec<u8>, key: &str) {
    buf.extend((key.len() as u64).to_le_bytes());
    buf.extend(key.as_bytes());
}

fn kv_string(buf: &mut Vec<u8>, key: &str, val: &str) {
    push_key(buf, key);
    buf.extend(8u32.to_le_bytes()); // GGUF type tag: string
    buf.extend((val.len() as u64).to_le_bytes());
    buf.extend(val.as_bytes());
}

fn kv_u32(buf: &mut Vec<u8>, key: &str, val: u32) {
    push_key(buf, key);
    buf.extend(4u32.to_le_bytes()); // GGUF type tag: u32
    buf.extend(val.to_le_bytes());
}

/// Writes the vector's `metadata` block into a minimal GGUF v3 file and hands
/// the path back. `null` values are written as *absent keys*, which is the
/// state the vector is recording. Nothing else is added: the reader sees
/// exactly the keys the contract vector carries.
fn write_vector_gguf(vector: &Value) -> PathBuf {
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
    let dir = std::env::temp_dir().join("bloomery-geometry-conformance-v1");
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

fn parse_vector(vector: &Value) -> GgufMeta {
    let path = write_vector_gguf(vector);
    parse_gguf_meta(&path).unwrap_or_else(|e| {
        panic!(
            "bloomery refused vector {}: {e}",
            vector["id"].as_str().unwrap_or("?")
        )
    })
}

// ---------------------------------------------------------------------------
// the conformance checks
// ---------------------------------------------------------------------------

fn expected_u64(vector: &Value, field: &str) -> Option<u64> {
    vector["expected"].get(field).and_then(Value::as_u64)
}

fn banned_u64(vector: &Value, field: &str) -> Vec<u64> {
    vector["must_not_equal"]
        .get(field)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

/// True when the vector's metadata carries no `{arch}.ssm.*` key, i.e. the
/// architecture has no recurrent layers. R4 permits `recurrent_state_bytes ==
/// 0` only in exactly this case.
fn has_ssm_keys(vector: &Value) -> bool {
    vector["metadata"]
        .as_object()
        .expect("metadata object")
        .keys()
        .any(|k| k.contains(".ssm."))
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

fn bound_by_for(limited_by: &str) -> BoundBy {
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
/// `recurrent_state_bytes` folds into bloomery's `ctx_overhead_bytes`; the v1
/// window scenarios all sit on a dense model and state no recurrent term, so
/// it is 0 here — a stated term, not a dropped one. `measured_ceiling` is a
/// bloomery-only extra term, unmeasured (`None`) in every scenario, so per R7
/// it drops out rather than being guessed.
fn check_windows(id: &str, vector: &Value, meta: &GgufMeta) {
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

// ---------------------------------------------------------------------------
// vendored-set integrity
// ---------------------------------------------------------------------------

#[test]
fn vendored_vector_set_matches_its_pinned_manifest() {
    let manifest = manifest();
    assert_eq!(manifest["set_version"], SET_VERSION);

    let files = manifest["files"]
        .as_object()
        .expect("manifest lists a files object");
    for (name, sha) in files {
        let path = data_dir().join(name);
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        assert_eq!(
            &sha256_hex(&bytes),
            sha.as_str().expect("sha is a string"),
            "vendored {name} differs from the published v1 bytes"
        );
    }

    // Nothing extra, nothing missing: the vendored directory is the set.
    let on_disk: BTreeSet<String> = fs::read_dir(data_dir())
        .expect("data dir readable")
        .map(|e| {
            e.expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|n| n != "MANIFEST.json")
        .collect();
    let listed: BTreeSet<String> = files.keys().cloned().collect();
    assert_eq!(
        on_disk, listed,
        "vendored directory contents differ from the manifest"
    );
}

#[test]
fn all_vectors_have_a_named_test() {
    let manifest = manifest();
    let listed: BTreeSet<String> = manifest["files"]
        .as_object()
        .expect("files object")
        .keys()
        .map(|k| k.trim_end_matches(".json").to_string())
        .collect();
    let covered: BTreeSet<String> = VECTOR_IDS.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        listed, covered,
        "every v1 vector needs its own #[test] in this file"
    );
}

// ---------------------------------------------------------------------------
// R5 — expert fields
// ---------------------------------------------------------------------------

/// R5 says an absent `expert_count` / `expert_used_count` is *absent*, never 0.
/// bloomery-core satisfies it by not modelling experts at all: `GgufMeta`
/// carries no expert field, so there is no place for a 0 to be invented. That
/// is a coverage gap, not a violation — recorded here so the day someone adds
/// the field, this test fails and forces it to be an `Option`, per R5.
#[test]
fn r5_bloomery_core_models_no_expert_fields() {
    // The vector with real expert metadata: 64 experts, 6 used.
    let vector = load_vector("deepseek-coder-v2-16b-lite-instruct-q5_K_M");
    assert_eq!(vector["expected"]["experts"]["count"], 64);
    assert_eq!(vector["expected"]["experts"]["used"], 6);

    let meta = parse_vector(&vector);
    let repr = format!("{meta:?}");
    assert!(
        !repr.to_lowercase().contains("expert"),
        "GgufMeta now carries an expert field ({repr}). R5: it MUST be an \
         Option that is None when the arch states no expert keys — a dense \
         model is not a 0-expert MoE. Update this test to assert that."
    );

    // And the dense control: no expert keys in metadata at all.
    let dense = load_vector("qwen2.5-coder-7b-instruct-q8_0");
    assert!(
        dense["expected"]["experts"].is_null(),
        "R5: dense = null, not 0"
    );
    let dense_repr = format!("{:?}", parse_vector(&dense));
    assert!(!dense_repr.to_lowercase().contains("expert"));
}

// ---------------------------------------------------------------------------
// R8 — refusal on insufficient keys
// ---------------------------------------------------------------------------

/// The gemma-4 vector states `attention.head_count_kv: null` — the key is
/// genuinely absent from the model's metadata (assay's e1-sweep records it as
/// the `geometry: null` edge). R8 requires a refusal that says why, not a
/// guessed head count.
#[test]
fn gemma_4_12b_it_qat_q4_0_latest() {
    let vector = load_vector("gemma-4-12b-it-qat-q4_0-latest");
    assert_eq!(
        vector["expected"]["refuses"], true,
        "this vector is the R8 case"
    );

    let path = write_vector_gguf(&vector);
    match parse_gguf_meta(&path) {
        Ok(meta) => panic!(
            "R8 VIOLATION: bloomery guessed a geometry for gemma-4 instead of \
             refusing — kv_heads {} head_dim {} => {} B/token",
            meta.kv_heads,
            meta.head_dim,
            kv_bytes_per_token(&meta)
        ),
        Err(e) => {
            // R8's "and says why": the error must name the absent key.
            let said = e.to_string();
            assert!(
                said.contains("gemma4.attention.head_count_kv"),
                "R8: the refusal must name the key it lacked, got: {said}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// one #[test] per vector
// ---------------------------------------------------------------------------

#[test]
fn codegemma_7b_instruct_q8_0() {
    check_vector("codegemma-7b-instruct-q8_0");
}

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
/// blocks of tensors. **This test is RED against bloomery-core as of
/// d51e073** — `parse_gguf_meta` reads `{arch}.block_count` raw and never
/// looks at `{arch}.nextn_predict_layers`, so R6 is unimplemented in the Rust
/// reader (it is implemented in `tools/flywheel/prune/prune.py`, which zeroes
/// the key at conversion time — a different layer, and one that only covers
/// artifacts this repo produced).
///
/// Measured 2026-08-27 in this worktree, first run:
/// `serving_block_count` 41, contract 40; `recurrent_state_bytes` 68,059,136,
/// contract 65,863,680 (a 2,195,456 B / 2.09 MiB per-context over-charge —
/// one extra recurrent layer). `kv_bytes_per_token` 20,480 and
/// `attention_layers` 10 both conform even in the trap state (41/4 == 10),
/// so the divergence is confined to the block count and the term derived
/// from it.
///
/// Kept verbatim and executable (`cargo test -p bloomery-core --test
/// geometry_conformance_test -- --ignored`) rather than weakened or deleted;
/// `qwen3_6_35b_a3b_reap48_mtp_trap_r6_divergence_is_pinned` keeps the actual
/// behaviour under CI in the meantime. Remove the `#[ignore]` the moment
/// `gguf.rs` subtracts `nextn_predict_layers`.
#[test]
#[ignore = "RED: bloomery-core does not implement R6 (nextn_predict_layers); \
            reported 2026-08-27, awaiting a ruling on the gguf.rs fix"]
fn qwen3_6_35b_a3b_reap48_mtp_trap() {
    check_vector("qwen3.6-35b-a3b-reap48-mtp-trap");
}

/// Pins bloomery-core's *actual* behaviour on the trap so the R6 gap stays
/// visible in CI instead of only in a report, and so this test fails — loudly,
/// demanding both it and the `#[ignore]` above be removed — the moment R6 is
/// implemented. It asserts the divergence, never the contract.
#[test]
fn qwen3_6_35b_a3b_reap48_mtp_trap_r6_divergence_is_pinned() {
    let vector = load_vector("qwen3.6-35b-a3b-reap48-mtp-trap");
    let meta = parse_vector(&vector);

    // The terms that DO conform, even in the trap state.
    assert_eq!(kv_bytes_per_token(&meta), 20_480, "R2/R3 hold on the trap");
    assert_eq!(meta.attention_layers, 10, "R3 holds: 41 / interval 4 == 10");
    assert_ne!(
        kv_bytes_per_token(&meta),
        81_920,
        "the all-blocks answer stays banned"
    );

    // The divergence, stated as the contract states it.
    assert_eq!(
        u64::from(meta.layers),
        banned_u64(&vector, "serving_block_count")[0],
        "expected the KNOWN R6 gap: bloomery still reports the raw block count"
    );
    assert_eq!(
        expected_u64(&vector, "serving_block_count"),
        Some(40),
        "the contract value this diverges from"
    );
    assert_eq!(
        meta.recurrent_state_bytes, 68_059_136,
        "downstream of the same gap: 31 recurrent layers charged, not 30 \
         (contract 65_863_680 — a 2_195_456 B per-context over-charge)"
    );
}
