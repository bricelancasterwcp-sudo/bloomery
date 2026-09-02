//! gguf-geometry contract (vector set v3) conformance for bloomery-core.
//!
//! The vectors under `tests/data/gguf_geometry_v3/` are a byte-exact vendored
//! copy of `gguf-geometry/vectors/v3/`. That repo holds no implementation:
//! its expected values come from committed assay/bloomery evidence, never
//! from code. The rules are R1-R9 in its `SPEC.md`; each assertion names the
//! rule it proves.
//!
//! This file holds the set-wide checks: the vendored set's own integrity
//! (sha-pinned against the published `MANIFEST.json`, so a copy that has
//! drifted from the contract cannot be read by any test here), R5's absence
//! from `GgufMeta` at all, and R8's refusal on insufficient keys.
//!
//! **Split 2026-09-01** (carried-debt slice D): the per-vector conformance
//! tests -- one `#[test]` per vector, which is what made this file 837 lines
//! -- are in `geometry_conformance_vectors_test.rs`. The loader, the minimal
//! GGUF v3 synthesiser and the conformance checks they share are in
//! `tests/common/gguf_vectors.rs`.

mod common;

use std::collections::BTreeSet;
use std::fs;

use bloomery_core::geometry::kv_bytes_per_token;
use bloomery_core::gguf::parse_gguf_meta;

use common::gguf_vectors::{
    data_dir, load_vector, manifest, parse_vector, sha256_hex, write_vector_gguf,
};

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
