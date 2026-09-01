//! Native HTTP API: the two operator routes that touch admission.
//!
//! `bless` (drift-watch design §2) files the current profile of model M as
//! its baseline; `unblock` (verdict-gated-admission design §4) clears THIS
//! boot's admission block without touching the reading or the baseline. They
//! share this file because they share fixtures and because the load-bearing
//! test is that neither one does the other's job.
//!
//! Split out of `api_native_test.rs` on 2026-09-01 (carried-debt slice D).

mod common;

use std::path::{Path, PathBuf};

use common::http;
use common::native::{profile_doc, serve_drift_blocked_qwen};

/// [`serve_drift_blocked_qwen`], but also wires the profiles directory and
/// files `qwen`'s current profile on disk — the fixture "bless does not
/// unblock" needs to observe something real over HTTP: `bless` reads
/// `profiles_dir/qwen.json` from disk (`ProfileStore::bless`), and
/// `serve_drift_blocked_qwen` alone wires no profiles directory at all, so a
/// bless against it would 500 before the property could even be asked
/// about.
fn serve_drift_blocked_qwen_with_profiles() -> (u16, bloomery_daemon::http::ServerHandle, PathBuf) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-drift-blocked-profiled-test-{}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(dir.join("profiles")).expect("scratch dir");
    std::fs::write(
        dir.join("profiles").join("qwen.json"),
        profile_doc("qwen", 2048),
    )
    .unwrap();

    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut pager = bloomery_daemon::pager::Pager::new(
        bloomery_substrate::fake::FakeSubstrate::new(),
        journal,
        images,
        Box::new(|| Some(1024 * 1024 * 1024)),
    );
    pager.set_profiles_dir(dir.join("profiles"));
    let gguf = dir.join("qwen.gguf");
    std::fs::write(&gguf, b"weights").unwrap();
    let meta = bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
        value_length: None,
        recurrent_state_bytes: 0,
    };
    pager.register_model("qwen", &gguf, meta, None).unwrap();
    pager
        .attach_profile(
            "qwen",
            bloomery_core::profile::Profile::from_json(&profile_doc("qwen", 2048))
                .expect("fixture profile parses"),
            false,
        )
        .unwrap();
    pager
        .set_drift(
            "qwen",
            bloomery_daemon::drift::ModelDrift {
                step: bloomery_daemon::drift::DriftStatus::WithinNoise,
                cumulative: bloomery_daemon::drift::DriftStatus::Confirmed {
                    reference: "base42".to_string(),
                },
            },
        )
        .unwrap();

    let (port, mut handle) = bloomery_daemon::http::serve(pager, 0);
    handle.set_scratch_dir(dir.clone());
    (port, handle, dir)
}
// ---------------------------------------------------------------------------

/// Builds and serves a `Pager<FakeSubstrate>` with `qwen` registered and —
/// when `wire_profiles_dir` — the profiles directory `main.rs` wires from
/// `config.data_dir/profiles`. Returns the scratch dir: the profiles directory
/// is `dir/profiles` and the boot journal is `dir/j.jsonl`.
///
/// Built from the same public pieces `serve_panicking` uses rather than from
/// `test_support::serve_fake`, which wires no profiles directory at all — and
/// `wire_profiles_dir: false` is exactly that daemon, the one this route must
/// refuse rather than serve by writing a baseline somewhere of its own
/// choosing.
fn serve_with_profiles(
    wire_profiles_dir: bool,
) -> (u16, bloomery_daemon::http::ServerHandle, PathBuf) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("bloomery-bless-test-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(dir.join("profiles")).expect("scratch dir");

    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut pager = bloomery_daemon::pager::Pager::new(
        bloomery_substrate::fake::FakeSubstrate::new(),
        journal,
        images,
        Box::new(|| Some(1024 * 1024 * 1024)),
    );
    if wire_profiles_dir {
        pager.set_profiles_dir(dir.join("profiles"));
    }
    let gguf = dir.join("qwen.gguf");
    std::fs::write(&gguf, b"weights").unwrap();
    let meta = bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
        value_length: None,
        recurrent_state_bytes: 0,
    };
    pager.register_model("qwen", &gguf, meta, None).unwrap();

    let (port, mut handle) = bloomery_daemon::http::serve(pager, 0);
    handle.set_scratch_dir(dir.clone());
    (port, handle, dir)
}

/// Every `Blessed` row in the fixture's journal as
/// `(model, profile_path, sha, provenance)`.
fn blessed_rows(dir: &Path) -> Vec<(String, String, String, String)> {
    bloomery_core::journal::replay(&dir.join("j.jsonl"))
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            bloomery_core::journal::Event::Blessed {
                model,
                profile_path,
                sha,
                provenance,
            } => Some((
                model.clone(),
                profile_path.clone(),
                sha.clone(),
                provenance.clone(),
            )),
            _ => None,
        })
        .collect()
}

/// 200: the blessing copies this boot's profile to the baseline, answers with
/// the identity of the bytes that landed there, and journals `operator` as the
/// provenance — design §2's "the provenance of every baseline is explicit".
#[test]
fn blessing_a_current_profile_answers_its_identity_and_journals_the_operator() {
    let (port, handle, dir) = serve_with_profiles(true);
    let addr = format!("127.0.0.1:{port}");
    let profiles = dir.join("profiles");
    let doc = profile_doc("qwen", 2048);
    std::fs::write(profiles.join("qwen.json"), &doc).unwrap();

    let (st, body) = http(&addr, "POST", "/models/qwen/bless", "");
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let baseline = profiles.join("qwen.baseline.json");
    assert_eq!(v["model"], "qwen");
    assert_eq!(
        v["sha"],
        bloomery_core::journal::sha256_hex(&doc),
        "the sha is of the blessed bytes, so `sha256sum` on the path checks it"
    );
    assert_eq!(v["path"], baseline.display().to_string());
    assert_eq!(std::fs::read_to_string(&baseline).unwrap(), doc);
    assert!(
        profiles.join("qwen.json").exists(),
        "blessing copies the current profile, it does not consume it"
    );

    let rows = blessed_rows(&dir);
    assert_eq!(rows.len(), 1, "one blessing, one row: {rows:?}");
    assert_eq!(rows[0].0, "qwen");
    assert_eq!(rows[0].1, baseline.display().to_string());
    assert_eq!(rows[0].2, bloomery_core::journal::sha256_hex(&doc));
    assert_eq!(rows[0].3, "operator");
    handle.shutdown();
}

/// Design §2: "Re-blessing replaces the baseline and journals the old identity
/// beside the new." The replaced document's bytes are gone — overwritten by
/// this blessing — so its digest in the row is all that is left of it, and it
/// is what ties this row back to the earlier `Blessed` row that named the same
/// digest.
#[test]
fn re_blessing_replaces_the_baseline_and_journals_the_replaced_identity() {
    let (port, handle, dir) = serve_with_profiles(true);
    let addr = format!("127.0.0.1:{port}");
    let profiles = dir.join("profiles");
    let old = profile_doc("qwen", 1024);
    let new = profile_doc("qwen", 4096);
    std::fs::write(profiles.join("qwen.baseline.json"), &old).unwrap();
    std::fs::write(profiles.join("qwen.json"), &new).unwrap();

    let (st, body) = http(&addr, "POST", "/models/qwen/bless", "");
    assert_eq!(st, 200, "{body}");
    assert_eq!(
        std::fs::read_to_string(profiles.join("qwen.baseline.json")).unwrap(),
        new,
        "re-blessing replaces the baseline"
    );

    let rows = blessed_rows(&dir);
    assert_eq!(rows.len(), 1, "one blessing, one row: {rows:?}");
    assert_eq!(rows[0].2, bloomery_core::journal::sha256_hex(&new));
    assert_eq!(
        rows[0].3,
        format!(
            "operator (replaced {})",
            bloomery_core::journal::sha256_hex(&old)
        ),
        "the identity the blessing overwrote is journaled beside the new one"
    );
    handle.shutdown();
}

/// 404: a name this daemon was never configured with. Same body shape as every
/// other unknown-model refusal on this surface, and nothing is written — a
/// route that filed a baseline for a model the pager does not serve would be
/// inventing evidence about a model nobody measured.
#[test]
fn blessing_an_unknown_model_returns_404_and_files_nothing() {
    let (port, handle, dir) = serve_with_profiles(true);
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "POST", "/models/does-not-exist/bless", "");
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "unknown_model");
    assert_eq!(v["model"], "does-not-exist");
    assert!(
        blessed_rows(&dir).is_empty(),
        "a refused blessing journals nothing"
    );
    assert!(!dir
        .join("profiles")
        .join("does-not-exist.baseline.json")
        .exists());
    handle.shutdown();
}

/// 409: there is no current profile to bless (POST never ran, or it failed for
/// this model). Named and refused — never a silent no-op, and never a 200 that
/// would tell an operator a baseline exists when nothing was written.
#[test]
fn blessing_with_no_current_profile_is_a_named_409_not_a_silent_no_op() {
    let (port, handle, dir) = serve_with_profiles(true);
    let addr = format!("127.0.0.1:{port}");
    let profiles = dir.join("profiles");

    let (st, body) = http(&addr, "POST", "/models/qwen/bless", "");
    assert_eq!(st, 409, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "no_current_profile");
    assert_eq!(v["model"], "qwen");
    let detail = v["detail"].as_str().unwrap_or_default().to_string();
    assert!(
        detail.contains("nothing to bless")
            && detail.contains(&profiles.join("qwen.json").display().to_string()),
        "the refusal names the document it looked for: {detail}"
    );
    assert!(
        !profiles.join("qwen.baseline.json").exists(),
        "a failed blessing writes no baseline"
    );
    assert!(
        blessed_rows(&dir).is_empty(),
        "a refused blessing journals nothing"
    );
    handle.shutdown();
}

/// A daemon with no profiles directory wired refuses by name rather than
/// blessing into whatever directory it happens to be running in. Unreachable
/// through `main.rs` (which always wires one), which is exactly why it is
/// pinned: the failure mode of a default here is a baseline nobody can find.
#[test]
fn blessing_without_a_configured_profiles_directory_is_a_named_500() {
    let (port, handle, dir) = serve_with_profiles(false);
    let addr = format!("127.0.0.1:{port}");
    std::fs::write(
        dir.join("profiles").join("qwen.json"),
        profile_doc("qwen", 2048),
    )
    .unwrap();

    let (st, body) = http(&addr, "POST", "/models/qwen/bless", "");
    assert_eq!(st, 500, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "internal");
    assert!(
        v["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("profiles directory"),
        "{body}"
    );
    assert!(
        blessed_rows(&dir).is_empty(),
        "a refused blessing journals nothing"
    );
    assert!(!dir.join("profiles").join("qwen.baseline.json").exists());
    handle.shutdown();
}

/// The route table's `_ => 404` still catches everything the new arm does not:
/// a neighbouring verb under `/models/{name}/` and the same path under the
/// wrong method are `not_found`, not blessings.
#[test]
fn a_neighbouring_path_or_the_wrong_method_still_falls_through_to_not_found() {
    let (port, handle, dir) = serve_with_profiles(true);
    let addr = format!("127.0.0.1:{port}");
    std::fs::write(
        dir.join("profiles").join("qwen.json"),
        profile_doc("qwen", 2048),
    )
    .unwrap();

    for (method, path) in [
        ("POST", "/models/qwen/blessing"),
        ("GET", "/models/qwen/bless"),
        ("POST", "/models/qwen/bless/again"),
    ] {
        let (st, body) = http(&addr, method, path, "");
        assert_eq!(st, 404, "{method} {path}: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["error"], "not_found", "{method} {path}");
    }
    assert!(
        blessed_rows(&dir).is_empty(),
        "no near-miss request blessed anything"
    );
    assert!(!dir.join("profiles").join("qwen.baseline.json").exists());
    handle.shutdown();
}

// ---------------------------------------------------------------------------
// The operator unblock route (verdict-gated-admission design §4): "I know,
// let it run anyway" — clears THIS boot's admission block without touching
// the reading or the blessed baseline. Neither this route nor `bless`
// implies the other.
// ---------------------------------------------------------------------------

/// Every `Admission` row in the fixture's journal as
/// `(model, action, reference, provenance)`.
fn admission_rows(dir: &Path) -> Vec<(String, String, String, String)> {
    bloomery_core::journal::replay(&dir.join("j.jsonl"))
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            bloomery_core::journal::Event::Admission {
                model,
                action,
                reference,
                provenance,
            } => Some((
                model.clone(),
                action.clone(),
                reference.clone(),
                provenance.clone(),
            )),
            _ => None,
        })
        .collect()
}

/// 200: clearing a standing block answers with what was cleared, journals
/// `"cleared"` with operator provenance, and admits new agents against the
/// model again — while the drift reading itself, still `Confirmed`, is left
/// exactly as measured.
#[test]
fn unblocking_a_blocked_model_admits_and_journals_the_operator() {
    let (port, handle) = serve_drift_blocked_qwen();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "POST", "/models/qwen/unblock", "");
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["model"], "qwen");
    assert_eq!(v["cleared"]["reference"], "base42");

    // Admission is open again…
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");

    // …and the reading itself is untouched.
    let (st, body) = http(&addr, "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let status: serde_json::Value = serde_json::from_str(&body).unwrap();
    let model = status["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "qwen")
        .unwrap();
    assert_eq!(
        model["drift"]["cumulative"]["status"], "confirmed",
        "{model}"
    );

    handle.shutdown();
}

/// 404: a name this daemon was never configured with. Same body shape as
/// every other unknown-model refusal on this surface.
#[test]
fn unblocking_an_unknown_model_returns_404() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "POST", "/models/does-not-exist/unblock", "");
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "unknown_model");
    assert_eq!(v["model"], "does-not-exist");
    handle.shutdown();
}

/// 409: a known, unblocked model. Answering 200 here would tell an operator
/// they cleared something when nothing was written — the silent no-op
/// design §4 forbids, the same reason `bless`'s 409 exists.
#[test]
fn unblocking_a_model_with_no_standing_block_is_a_named_409_not_a_silent_no_op() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "POST", "/models/qwen/unblock", "");
    assert_eq!(st, 409, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "no_admission_block");
    assert_eq!(v["model"], "qwen");
    assert!(
        v["detail"].as_str().is_some_and(|d| !d.is_empty()),
        "{body}"
    );
    handle.shutdown();
}

/// Unblocking does not rebaseline: it takes the block down without filing a
/// new baseline anywhere, so there is nothing for a next-boot comparison to
/// read differently. And a bless on a blocked model does not, on its own,
/// admit anything this boot — the two routes answer different questions.
///
/// The fixture is a model that IS blocked
/// (`serve_drift_blocked_qwen_with_profiles`), not `serve_with_profiles`'s
/// unblocked one: against an unblocked model, "bless does not unblock" is
/// unobservable — there is nothing standing for a bless to (not) clear, so
/// the property can only be pinned by watching a real block survive a bless.
#[test]
fn unblock_does_not_bless_and_bless_does_not_unblock_over_http() {
    let (port, handle, dir) = serve_drift_blocked_qwen_with_profiles();
    let addr = format!("127.0.0.1:{port}");

    // The block stands before either route is touched.
    let block_reference = |body: &str| -> serde_json::Value {
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        v["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == "qwen")
            .unwrap()["admission_block"]
            .clone()
    };
    let (st, body) = http(&addr, "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    assert_eq!(block_reference(&body)["reference"], "base42", "{body}");
    // The fixture's own `set_drift` already journaled the "blocked" row that
    // put this block there — captured here so the next check is "bless adds
    // no row of its own", not the wrong claim "there is no row at all".
    let rows_before_bless = admission_rows(&dir);

    // Bless does not unblock: the block stands after a bless…
    let (st, body) = http(&addr, "POST", "/models/qwen/bless", "");
    assert_eq!(st, 200, "{body}");
    assert_eq!(
        admission_rows(&dir),
        rows_before_bless,
        "bless journals no Admission row of its own"
    );
    let (st, body) = http(&addr, "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    assert_eq!(
        block_reference(&body)["reference"],
        "base42",
        "bless must not clear the standing block: {body}"
    );

    // …observably: new agents are still refused after the bless.
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(
        st, 422,
        "a bless on a blocked model must not, on its own, admit anything this boot: {body}"
    );

    // Unblock does not rebaseline: the baseline bytes bless just wrote are
    // untouched by the unblock that follows.
    let baseline = dir.join("profiles").join("qwen.baseline.json");
    assert!(baseline.exists());
    let before = std::fs::read(&baseline).unwrap();
    let (st, body) = http(&addr, "POST", "/models/qwen/unblock", "");
    assert_eq!(st, 200, "{body}");
    assert_eq!(
        std::fs::read(&baseline).unwrap(),
        before,
        "unblock must not touch the blessed baseline"
    );

    // And now that unblock actually ran, admission is open again — the
    // fixture's block is gone, not merely unobserved.
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");

    handle.shutdown();
}

/// The route table's `_ => 404` still catches a neighbouring path or the
/// wrong method.
#[test]
fn unblock_neighbouring_path_or_wrong_method_falls_through_to_not_found() {
    let (port, handle) = serve_drift_blocked_qwen();
    let addr = format!("127.0.0.1:{port}");

    for (method, path) in [
        ("POST", "/models/qwen/unblocking"),
        ("GET", "/models/qwen/unblock"),
        ("POST", "/models/qwen/unblock/again"),
    ] {
        let (st, body) = http(&addr, method, path, "");
        assert_eq!(st, 404, "{method} {path}: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["error"], "not_found", "{method} {path}");
    }
    handle.shutdown();
}

/// The tier an operator declared is what every profile in this daemon is
/// marked with, so `/status` has to say which one it is — `null` when the
/// daemon was never told, never an invented name.
#[test]
fn status_reports_the_declared_tier() {
    let (port, handle) =
        bloomery_daemon::test_support::serve_fake_with_tier("mid-gamer-12gb", true);
    let (st, body) = http(&format!("127.0.0.1:{port}"), "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["tier"]["name"], "mid-gamer-12gb");
    assert_eq!(v["tier"]["emulated"], true);
    handle.shutdown();
}
