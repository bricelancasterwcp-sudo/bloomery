//! gguf-geometry contract (vector set v3) conformance for bloomery-core.
//!
//! The vectors under `tests/data/gguf_geometry_v3/` are a byte-exact vendored
//! copy of `gguf-geometry/vectors/v3/`, vendored 2026-08-28 from gguf-geometry
//! master `84f042b` (public CI green, run 33163833319; verified with `cmp`
//! file-by-file and by sha256 against the published `MANIFEST.json`). That
//! repo holds no implementation: its expected values come from committed
//! assay/bloomery evidence, never from code. The rules are R1-R9 in its
//! `SPEC.md`; each assertion below names the rule it proves.
//!
//! Per that repo's consumer model, exactly one set is vendored at a time: v3
//! replaced the previously vendored v2 here, and `tests/data/gguf_geometry_v2/`
//! was deleted rather than kept alongside. `vectors/v2/` stays frozen upstream.
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
//! # Divergence found and resolved (2026-08-27, against set v1)
//!
//! On first run 9 of the 10 v1 vectors conformed. The tenth,
//! `qwen3.6-35b-a3b-reap48-mtp-trap`, was RED: bloomery-core did not implement
//! **R6** — `parse_gguf_meta` read `{arch}.block_count` raw and never
//! subtracted `{arch}.nextn_predict_layers`, over-charging one recurrent layer
//! (2,195,456 B per context) on a trapped GGUF. `gguf.rs::resolve_serving_block_count`
//! implements R6 as of that arc and all 10 vectors conformed; see
//! `qwen3_6_35b_a3b_reap48_mtp_trap` and
//! `qwen3_6_35b_a3b_reap48_mtp_trap_r6_conformance_is_pinned`, plus the unit
//! tests for the key's present/zero/absent/nonsense cases in `gguf_test.rs`.
//! R6's rule text is unchanged in v2, and that vector's bytes carried forward
//! into v2 identically, so those two tests are the same checks on the same
//! inputs.
//!
//! # What v2 adds (2026-08-27)
//!
//! Eleven vectors instead of ten. Eight carry forward byte-for-byte (identical
//! shas across the two sets); two carry metadata-only additions that move no
//! `expected` value — gemma-4 gains its sliding-window / `*_swa` family (still
//! the R8 refusal case, since `attention.head_count_kv` is still absent) and
//! deepseek-coder-v2 gains `attention.value_length` 128 against its
//! `key_length` 192.
//!
//! That deepseek addition is upstream's recorded-but-unclosed R2/MLA gap
//! (SPEC.md, "Known open gap"): R2 is specified for `key_length` alone, and
//! bloomery derives `head_dim` from `key_length` only (R1), so the extra key is
//! inert here and the vector's hardware-verified 331,776 B/token still passes.
//! An implementation that ever models K and V widths separately would fail that
//! vector — deliberately, until a rule and a measurement close the gap.
//!
//! The eleventh vector is new: `qwen3.8-27b` (`qwen35`, 65 blocks,
//! `full_attention_interval` 4, `nextn_predict_layers` 1, `ssm.*`), the case v1
//! withheld rather than pin at assay's then-published 266,240 B/token — a
//! 4.0625x attention over-charge. It exercises R3, R4 and R6 together in one
//! model, which is exactly the combination `resolve_serving_block_count` ->
//! `resolve_attention_layers` -> `resolve_recurrent_state_bytes` computes, and
//! it conformed through bloomery's own reader on the first run of this
//! re-vendor with no production change.
//!
//! # Divergence found and resolved (2026-08-28, against set v2)
//!
//! Task 9 (commit `3e326c6`, same branch) landed R9 — `kv_bytes_per_token`
//! now reads `GgufMeta.value_length` and, when it is stated and differs from
//! `head_dim`, replaces the dense "2x head_dim" factor with the explicit K+V
//! sum. That flipped the still-v2-vendored deepseek vector from passing to
//! RED: its pinned `expected.kv_bytes_per_token` was 331,776 (`key_length`
//! 192 charged for both K and V — R2 arithmetic applied to stated metadata,
//! never an observed allocation), but R9 now computes 276,480 from the same
//! vector's `metadata` (`key_length` 192, `value_length` 128) through
//! `parse_gguf_meta`. The vector bit through the reader exactly as designed:
//! a real production change made a real pinned value wrong, and the test
//! caught it before this re-vendor closed the gap.
//!
//! # What v3 changes (2026-08-28)
//!
//! Ten of the eleven vectors carry forward byte-for-byte (identical shas
//! across v2 and v3). Only `deepseek-coder-v2-16b-lite-instruct-q5_K_M`
//! moves, and only its `expected`/`must_not_equal` blocks — its `metadata`
//! (`key_length` 192, `value_length` 128) is unchanged from v2, so R9 was
//! already exercising both keys through `parse_gguf_meta` before this
//! re-vendor; what changes is which answer the vector calls correct.
//! `expected.kv_bytes_per_token` is re-pinned from 331,776 to 276,480 — the
//! measured KV allocation (assay `docs/superpowers/evidence/mla-kv-2026-08-27/`,
//! ollama 0.32.13 llama runner, non-FA lens, exact and identical at all three
//! measured ctx points, verdict H-b), not a recomputation of the old value.
//! `must_not_equal.kv_bytes_per_token` grows to four disproved candidates:
//! the pre-R1 formula (221,184), the pre-R9 k-width-for-both pin this vector
//! carried through v1 and v2 (331,776), and two MLA-latent guesses disproved
//! by the same measurement (31,104 and 62,208) — see gguf-geometry SPEC.md R9
//! and `docs/upstream-errata/2026-08-27-assay-deepseek-mla-kv-overcharge.md`.
//! `tests/data/gguf_geometry_v2/` is deleted in the same commit per the
//! one-set consumer model described above; `vectors/v2/` stays frozen
//! upstream.

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
/// (2026-08-28) from `gguf-geometry/vectors/v3/MANIFEST.json` at master
/// `84f042b`. The manifest in turn pins every vector file, so this single
/// constant pins the whole set: a vendored copy that has drifted from the
/// published contract cannot be read by any test in this file.
///
/// Set history: v1's manifest hashed
/// `44f8af208d4bd79055cdaa9e0b9c4e9fa81f305d5f81ab6e87457de6c3fa470a`; v2's
/// hashed `06da801b5dc57fedbfd42555c377c1cd3b6b8fb3c549cd2d367396447fc15116`.
/// A new set is a visible diff here, which is the point of the pin.
const MANIFEST_SHA256: &str = "c4d5c22d99e658e21d7197fffe915969a9a1d1fe62683a9c3e7d85b884798e4b";

const SET_VERSION: &str = "v3";

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
    "qwen3.8-27b",
];

// ---------------------------------------------------------------------------
// vendored-set loading (sha-pinned)
// ---------------------------------------------------------------------------

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/gguf_geometry_v3")
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
         copy has drifted from gguf-geometry vectors/v3 (re-vendor byte-exact, \
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
/// `recurrent_state_bytes` folds into bloomery's `ctx_overhead_bytes`; in v2 as
/// in v1 the only window scenarios in the set are qwen2.5-coder-7b's three,
/// which sit on a dense model and state no recurrent term, so it is 0 here — a
/// stated term, not a dropped one. (`qwen3.8-27b` states no windows at all:
/// upstream records that its erratum's conforming window figures stay
/// condition-parameterized under a box state no later run can re-occupy.)
/// `measured_ceiling` is a
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
            "vendored {name} differs from the published v3 bytes"
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
        "every v3 vector needs its own #[test] in this file"
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
