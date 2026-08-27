//! The capture seam's binding tests (memory-organ Task 3 brief; spec
//! `docs/superpowers/specs/2026-08-26-memory-organ-design.md` §2) — the
//! brief's three, plus `a_patch_that_does_not_land_captures_nothing`, which
//! covers a stated behavior the brief's three cannot reach (see its own doc
//! comment).
//!
//! Every test drives the REAL `run_task` against a scripted `FakeSubstrate`
//! — the same GPU-free fixture pattern `task_loop_test.rs` and
//! `task/registry.rs`'s own tests use — because the properties under test
//! (first-touch pinning, which steps contribute evidence, and which do not)
//! live in the loop's dispatch, not in any executor called alone. A unit
//! test of `exec_read`/`exec_patch` in isolation could not observe
//! `or_insert` at all.
//!
//! **Why `.txt` fixtures rather than `.py`:** `exec_patch` picks its landing
//! lens by extension, and `PythonLens` shells out to a real `python3`
//! (fail-closed when absent — see `lens_py`). A `.py` fixture would make
//! these tests pass or skip depending on whether the box has an interpreter,
//! which says nothing about fingerprint capture: the seam under test is
//! lens-independent, and `task_exec_patch_test.rs` already owns the Python
//! lens's own coverage. `PlainText` keeps every assertion here deterministic.

use bloomery_core::action::{PatchBody, PatchCodec};
use bloomery_core::gguf::GgufMeta;
use bloomery_core::grant::Grant;
use bloomery_core::journal::{sha256_hex_bytes, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::{run_task, ExecBounds, PreTouch, TaskSpec, TaskStatus};
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A fresh, per-test tempdir — PID + atomic counter, so parallel test
/// threads in one `cargo test` process never collide. Copied from
/// `task/registry.rs`'s test helpers.
fn fresh_dir(tag: &str) -> PathBuf {
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-memcapture-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn meta() -> GgufMeta {
    GgufMeta {
        arch: "qwen2".into(),
        layers: 4,
        attention_layers: 4,
        kv_heads: 2,
        head_dim: 32,
        training_ctx: 65536,
        weights_bytes: 1000,
        recurrent_state_bytes: 0,
    }
}

fn build_pager(dir: &Path, replies: Vec<Reply>) -> (Pager<FakeSubstrate>, String) {
    let journal = Journal::open(&dir.join("pager.jsonl")).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    for r in replies {
        fake.script_reply(r);
    }
    let mut pager = Pager::new(fake, journal, images, Box::new(|| Some(1024 * 1024 * 1024)));
    let gguf = dir.join("m.gguf");
    std::fs::write(&gguf, b"fake weights").unwrap();
    pager.register_model("m", &gguf, meta(), None).unwrap();
    let info = pager.create_agent("m", 100, None, 1_000_000).unwrap();
    (pager, info.id)
}

fn ok_grant(dir: &Path) -> Grant {
    let sb = std::fs::canonicalize(dir).unwrap();
    Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}"],"commands":[]}}"#,
        s = sb.display()
    ))
    .unwrap()
}

fn scripted(text: &str) -> Reply {
    Reply {
        text: text.to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    }
}

/// A canonical sandbox under `dir` the grant is scoped to. Canonicalized
/// here so the test's expected keys are the same strings the executors'
/// grant checks return (macOS-style `/private` prefixes and symlinked
/// `/tmp` would otherwise diverge).
fn sandbox(dir: &Path) -> PathBuf {
    let sb = dir.join("sandbox");
    std::fs::create_dir_all(&sb).unwrap();
    std::fs::canonicalize(&sb).unwrap()
}

fn spec(grant: Grant, cwd: PathBuf, bounds: ExecBounds) -> TaskSpec {
    spec_with_codec(grant, cwd, bounds, PatchCodec::WholeFile)
}

/// [`spec`] with an explicit codec — the did-not-land test needs
/// `SearchReplace`, the one codec whose apply step can actually fail
/// (`WholeFile` always applies, by construction).
fn spec_with_codec(
    grant: Grant,
    cwd: PathBuf,
    bounds: ExecBounds,
    patch_codec: PatchCodec,
) -> TaskSpec {
    TaskSpec {
        goal: "capture evidence".to_string(),
        grant,
        budget_tokens: 1_000_000,
        max_steps: 5,
        cwd,
        patch_codec,
        bounds,
        mutating_verbs: true,
        envelope: EnvelopeLens::V1,
        memory_block: None,
        window_ladder: false,
    }
}

/// Spec §2: a cited file carries "the sha256 of its bytes **before the
/// task's first touch** of that path". This is the first-touch rule's own
/// test.
///
/// **Why the script patches twice.** The obvious script (read, then patch,
/// then done) cannot tell `or_insert` from `insert` at all: `exec_patch`
/// reads the *current* bytes before applying anything, so the first patch's
/// pre-touch fingerprint is byte-identical to the read's, and overwriting
/// it is invisible. It takes a SECOND patch — whose pre-read now sees the
/// first patch's output — for the two rules to disagree. This script was
/// mutation-checked against exactly that: replacing `or_insert` with
/// `insert` fails this test on the `mid` hash.
///
/// The landed patch bodies are asserted here too, as the other half of a
/// successful patch step's evidence: the path key must be the canonical
/// path (never the model's relative `"a.txt"`), the body must be the
/// decoded `PatchBody` verbatim, and the two must be in step order — spec
/// §2 stores them so an exact repeat can replay them, which a reordered
/// list would silently corrupt.
#[test]
fn read_then_patch_fingerprints_at_first_touch() {
    let dir = fresh_dir("first-touch");
    let sb = sandbox(&dir);
    const BEFORE: &[u8] = b"hello\nworld\n";
    std::fs::write(sb.join("a.txt"), BEFORE).unwrap();

    let (mut pager, agent_id) = build_pager(
        &dir,
        vec![
            scripted("<action verb=\"read\" path=\"a.txt\">\n</action>"),
            scripted("<action verb=\"patch\" path=\"a.txt\">\nmid\n</action>"),
            scripted("<action verb=\"patch\" path=\"a.txt\">\ngoodbye\n</action>"),
            scripted("<action verb=\"done\">\nall set\n</action>"),
        ],
    );
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = spec(ok_grant(&sb), sb.clone(), ExecBounds::default());

    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);

    assert_eq!(result.status, TaskStatus::Done, "{result:?}");
    assert!(
        result.steps.iter().all(|s| !s.failed),
        "every scripted step must succeed: {:?}",
        result.steps
    );
    // Both patches really landed, so the three candidate fingerprints
    // (read-time, after-`mid`, after-`goodbye`) are genuinely different
    // hashes — without this the assertions below could pass vacuously.
    assert_eq!(
        std::fs::read_to_string(sb.join("a.txt")).unwrap(),
        "goodbye"
    );

    let canon = sb.join("a.txt").display().to_string();
    assert_eq!(
        result.touched_files.len(),
        1,
        "one path touched three times is one citation: {:?}",
        result.touched_files
    );
    assert_eq!(
        result.touched_files.get(&canon),
        Some(&PreTouch::Sha256(sha256_hex_bytes(BEFORE))),
        "the fingerprint must be the FIRST touch's bytes"
    );
    assert_ne!(
        result.touched_files.get(&canon),
        Some(&PreTouch::Sha256(sha256_hex_bytes(b"mid"))),
        "the second patch's pre-read must not overwrite the first touch"
    );

    assert_eq!(
        result.landed_patches,
        vec![
            (
                canon.clone(),
                PatchBody::WholeFile {
                    contents: "mid".to_string()
                }
            ),
            (
                canon,
                PatchBody::WholeFile {
                    contents: "goodbye".to_string()
                }
            ),
        ],
    );
}

/// Spec §2: "a file the task created carries the distinguished fingerprint
/// `absent`", and the mint bar is built only from execution evidence — so a
/// refused step contributes nothing at all. Both halves are asserted in one
/// task because the failure they guard against is the same one: capturing
/// from the action rather than from the successful observation.
#[test]
fn patch_created_file_fingerprints_absent_and_failed_steps_capture_nothing() {
    let dir = fresh_dir("created-absent");
    let sb = sandbox(&dir);

    let (mut pager, agent_id) = build_pager(
        &dir,
        vec![
            scripted("<action verb=\"patch\" path=\"b.txt\">\ncreated\n</action>"),
            scripted("<action verb=\"read\" path=\"/etc/passwd\">\n</action>"),
            scripted("<action verb=\"done\">\nrefused as expected\n</action>"),
        ],
    );
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = spec(ok_grant(&sb), sb.clone(), ExecBounds::default());

    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);

    assert_eq!(result.status, TaskStatus::Done, "{result:?}");
    assert!(
        !result.steps[0].failed,
        "the patch must land: {:?}",
        result.steps[0]
    );
    assert!(
        result.steps[1].failed,
        "the out-of-root read must be refused: {:?}",
        result.steps[1]
    );

    let canon_b = sb.join("b.txt").display().to_string();
    assert_eq!(
        result.touched_files.len(),
        1,
        "the refused read must contribute no citation: {:?}",
        result.touched_files
    );
    assert_eq!(result.touched_files.get(&canon_b), Some(&PreTouch::Absent));
    assert_eq!(
        result.landed_patches,
        vec![(
            canon_b,
            PatchBody::WholeFile {
                contents: "created".to_string()
            }
        )],
    );
}

/// A patch whose search text does not match leaves the file's bytes exactly
/// as they were — the task never touched it — so it must cite nothing and
/// land nothing, even though `exec_patch` got far enough to compute a
/// perfectly good pre-touch fingerprint before `land()` refused. No other
/// test here can reach this state: every scripted patch elsewhere in this
/// file lands, and `WholeFile` (their codec) always applies by
/// construction.
///
/// **What this pins, precisely.** The property is enforced twice over —
/// `exec_patch` attaches the capture only inside `Landing::Lands`, AND
/// `run_task` gates the capture on `!obs.failed` — so no single-line
/// mutation flips it. That redundancy is deliberate (see both sites' own
/// comments), and this test is what proves the pair actually holds: it was
/// mutation-checked against removing BOTH defenses at once, and fails.
#[test]
fn a_patch_that_does_not_land_captures_nothing() {
    let dir = fresh_dir("did-not-land");
    let sb = sandbox(&dir);
    std::fs::write(sb.join("d.txt"), b"hello\n").unwrap();

    let (mut pager, agent_id) = build_pager(
        &dir,
        vec![
            scripted(
                "<action verb=\"patch\" path=\"d.txt\">\n\
                 <<<<<<< SEARCH\n\
                 not in this file\n\
                 =======\n\
                 replacement\n\
                 >>>>>>> REPLACE\n\
                 </action>",
            ),
            scripted("<action verb=\"done\">\ncould not apply\n</action>"),
        ],
    );
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = spec_with_codec(
        ok_grant(&sb),
        sb.clone(),
        ExecBounds::default(),
        PatchCodec::SearchReplace,
    );

    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);

    assert_eq!(result.status, TaskStatus::Done, "{result:?}");
    assert!(
        result.steps[0].failed && result.steps[0].outcome.contains("did not apply"),
        "the fixture must fail at the APPLY step, not earlier: {:?}",
        result.steps[0]
    );
    assert_eq!(
        std::fs::read_to_string(sb.join("d.txt")).unwrap(),
        "hello\n"
    );

    assert!(
        result.touched_files.is_empty(),
        "an attempted patch is not a touch: {:?}",
        result.touched_files
    );
    assert!(result.landed_patches.is_empty());
}

/// A read truncated at `read_cap_bytes` saw only a prefix of the file, so
/// no honest whole-file sha exists for it — spec §2 requires the citation
/// be the hash of the file's bytes, and a prefix hash is not that. The seam
/// records `Uncomputable` rather than a hash of what it happened to read;
/// Task 5's mint bar refuses to mint over it.
#[test]
fn truncated_read_is_uncomputable() {
    let dir = fresh_dir("truncated");
    let sb = sandbox(&dir);
    std::fs::write(sb.join("c.txt"), b"0123456789").unwrap();

    let (mut pager, agent_id) = build_pager(
        &dir,
        vec![
            scripted("<action verb=\"read\" path=\"c.txt\">\n</action>"),
            scripted("<action verb=\"done\">\nsaw a prefix\n</action>"),
        ],
    );
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let bounds = ExecBounds {
        read_cap_bytes: 4,
        ..ExecBounds::default()
    };
    let spec = spec(ok_grant(&sb), sb.clone(), bounds);

    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);

    assert_eq!(result.status, TaskStatus::Done, "{result:?}");
    assert!(
        result.steps[0].outcome.contains("truncated at cap"),
        "the fixture must actually truncate: {:?}",
        result.steps[0]
    );

    let canon_c = sb.join("c.txt").display().to_string();
    assert_eq!(
        result.touched_files.get(&canon_c),
        Some(&PreTouch::Uncomputable),
        "a prefix read must never be recorded as a whole-file sha"
    );
    assert!(result.landed_patches.is_empty());
}
