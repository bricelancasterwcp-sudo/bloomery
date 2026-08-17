//! Profile retention (spec §5) and the drift gate (spec §3-§4)
//! (`docs/superpowers/specs/2026-08-17-drift-watch-design.md`).
//!
//! No assay, no python, no GPU anywhere here. The retention half is
//! filesystem-only; the gate half drives the subprocess seam POST already
//! practices (an injected `CommandRunner`), so every exit code, every refusal
//! and the timeout path are exercised without assay ever being installed. Two
//! deliberate exceptions spawn a real child: the first test runs the *real*
//! `run_post` against a fake assay so the store's idea of "the current
//! profile" is pinned to the file POST actually writes, and the wedged-diff
//! test runs `/bin/sh` under the real bounded-spawn layer.

use bloomery_core::journal::{replay, sha256_hex, sha256_hex_bytes, Event, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::{load_config, Tier};
use bloomery_daemon::drift::{
    diff_argv, drift_event, profile_file_name, Comparison, DriftError, DriftGate, GateOutcome,
    ProfileStore, Rotation, DIFF_TIMEOUT_SECS, MAX_TRANSIENTS,
};
use bloomery_daemon::pager::Pager;
use bloomery_daemon::post::PostRunner;
use bloomery_substrate::fake::FakeSubstrate;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A scratch directory unique to this process and this call, so the
/// integration binary's concurrently-running tests never share one.
fn scratch(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("bloomery-drift-{}-{seq}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A scratch dir plus the `profiles/` subdirectory `main.rs` creates, and a
/// store rooted at it.
fn store_in(tag: &str) -> (PathBuf, PathBuf, ProfileStore) {
    let dir = scratch(tag);
    let profiles = dir.join("profiles");
    std::fs::create_dir_all(&profiles).expect("profiles dir");
    let store = ProfileStore::new(&profiles);
    (dir, profiles, store)
}

/// A minimal but real assay profile document — the same shape `post_test.rs`
/// feeds its fake assay, so what parses here parses there.
fn profile_doc(model: &str) -> String {
    format!(
        r#"{{"assay_profile_version":3,"probe_version":"0.4.1","model":{{"name":"{model}"}},"ceiling":{{"max_verified":2048}},"verdicts":{{}}}}"#
    )
}

fn set_mtime(path: &Path, t: SystemTime) {
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for set_times");
    f.set_times(std::fs::FileTimes::new().set_modified(t))
        .expect("set mtime");
}

fn mtime(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .expect("metadata")
        .modified()
        .expect("mtime")
}

// ---------------------------------------------------------------------------
// Naming: one rule, shared with POST
// ---------------------------------------------------------------------------

/// The load-bearing agreement: whatever POST writes this boot is exactly what
/// [`ProfileStore::paths`] calls `current`. Driven through the real
/// `run_post` — a fake assay writing to whatever `--json` path it is handed,
/// exactly as assay does — so this is a behavioural pin, not a restatement of
/// the format string.
#[test]
fn the_stores_current_path_is_the_file_post_actually_writes() {
    let (dir, profiles, store) = store_in("post-agreement");
    let jpath = dir.join("j.jsonl");
    let mut pager = Pager::new(
        FakeSubstrate::new(),
        Journal::open(&jpath).expect("journal"),
        ImageStore::new(&dir.join("img")).expect("image store"),
        Box::new(|| Some(10u64.pow(9))),
    );
    let gguf = dir.join("qwen.gguf");
    std::fs::write(&gguf, b"weights").unwrap();
    pager
        .register_model("qwen", &gguf, qwen_like_meta(), None)
        .unwrap();
    pager.set_posting(true);
    let pager = std::sync::Mutex::new(pager);

    bloomery_daemon::post::run_post(
        &pager,
        &scripted_assay(),
        &["qwen".to_string()],
        8181,
        &tier(),
        &profiles,
    )
    .expect("POST runs");

    let current = store.paths("qwen").current;
    assert_eq!(
        std::fs::read_to_string(&current).expect("POST wrote the store's current path"),
        profile_doc("qwen")
    );

    // And the journal's path claim names the same file.
    let events = replay(&jpath).unwrap();
    let claimed = current.display().to_string();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Post { model, profile_path: Some(p), .. }
                if model == "qwen" && *p == claimed)),
        "expected a Post row naming {claimed}, got {events:?}"
    );
}

/// The retention siblings all sit beside the current document, in the
/// profiles root, and all derive from the one name POST writes.
#[test]
fn the_retention_siblings_sit_beside_the_current_document() {
    let (_dir, profiles, store) = store_in("paths");
    let p = store.paths("qwen2.5-coder:7b-q8_0");

    assert_eq!(
        p.current,
        profiles.join(profile_file_name("qwen2.5-coder:7b-q8_0"))
    );
    assert_eq!(
        p.previous,
        profiles.join("qwen2.5-coder:7b-q8_0.previous.json")
    );
    assert_eq!(
        p.baseline,
        profiles.join("qwen2.5-coder:7b-q8_0.baseline.json")
    );
}

// ---------------------------------------------------------------------------
// Rotation
// ---------------------------------------------------------------------------

#[test]
fn rotate_moves_current_to_previous_and_names_what_moved() {
    let (_dir, _profiles, store) = store_in("rotate-ok");
    let p = store.paths("qwen");
    std::fs::write(&p.current, profile_doc("qwen")).unwrap();

    match store.rotate("qwen").expect("rotate") {
        Rotation::Rotated { from, to } => {
            assert_eq!(from, p.current);
            assert_eq!(to, p.previous);
        }
        other => panic!("expected Rotated, got {other:?}"),
    }

    assert!(!p.current.exists(), "current is moved, not copied");
    assert_eq!(
        std::fs::read_to_string(&p.previous).unwrap(),
        profile_doc("qwen")
    );
}

/// The wrong caller order (POST's delete-before-probe running first) is not
/// silent: it shows up every boot as a rotation that moved nothing, and the
/// previous boot's profile survives untouched rather than being replaced by
/// whatever the probe just wrote.
#[test]
fn rotate_with_no_current_is_a_named_no_op_that_keeps_previous() {
    let (_dir, _profiles, store) = store_in("rotate-absent");
    let p = store.paths("qwen");
    std::fs::write(&p.previous, profile_doc("last-boot")).unwrap();

    match store.rotate("qwen").expect("rotate") {
        Rotation::NothingToRotate { current } => assert_eq!(current, p.current),
        other => panic!("expected NothingToRotate, got {other:?}"),
    }

    assert_eq!(
        std::fs::read_to_string(&p.previous).unwrap(),
        profile_doc("last-boot"),
        "an absent current must never clobber the previous boot's profile"
    );
}

/// Spec §5's rotation law: rotation happens **only** after the current file
/// parsed successfully. A boot that meets an unparseable current keeps the
/// previous reference it already had — a corrupt document must never be
/// promoted to "the previous boot's measurement".
#[test]
fn an_unparseable_current_is_not_rotated_and_leaves_previous_untouched() {
    let (_dir, _profiles, store) = store_in("rotate-unparseable");
    let p = store.paths("qwen");
    std::fs::write(&p.previous, profile_doc("good-reference")).unwrap();
    std::fs::write(&p.current, b"{ truncated json").unwrap();

    match store.rotate("qwen").expect("rotate") {
        Rotation::KeptUnparseable { current, reason } => {
            assert_eq!(current, p.current);
            assert!(!reason.is_empty(), "the refusal must say why");
        }
        other => panic!("expected KeptUnparseable, got {other:?}"),
    }

    assert_eq!(
        std::fs::read_to_string(&p.previous).unwrap(),
        profile_doc("good-reference"),
        "previous must survive a failed-parse boot"
    );
    assert!(
        p.current.exists(),
        "the unparseable document stays put for the operator to look at"
    );
}

/// The same law against the corruption mode that actually happens. The test
/// above writes `b"{ truncated json"`, which is valid UTF-8 and so only
/// exercises the JSON parser; a torn write leaves bytes that do not decode at
/// all (a NUL-filled block, a half-written multibyte sequence). Read through
/// `read_to_string` that is `io::Error(InvalidData)` — an `Err` return the
/// caller cannot tell apart from an unreadable disk, for precisely the
/// failure `KeptUnparseable` exists to name.
#[test]
fn a_non_utf8_current_is_kept_unparseable_rather_than_an_io_error() {
    let (_dir, _profiles, store) = store_in("rotate-non-utf8");
    let p = store.paths("qwen");
    std::fs::write(&p.previous, profile_doc("good-reference")).unwrap();
    std::fs::write(&p.current, b"\xff\xfe not utf8").unwrap();

    match store.rotate("qwen") {
        Ok(Rotation::KeptUnparseable { current, reason }) => {
            assert_eq!(current, p.current);
            assert!(
                reason.contains("UTF-8"),
                "the refusal must name the decode failure, got {reason:?}"
            );
        }
        other => panic!("expected Ok(KeptUnparseable), got {other:?}"),
    }

    assert_eq!(
        std::fs::read_to_string(&p.previous).unwrap(),
        profile_doc("good-reference"),
        "previous must survive an undecodable current"
    );
    assert!(p.current.exists());
}

// ---------------------------------------------------------------------------
// Blessing
// ---------------------------------------------------------------------------

#[test]
fn bless_copies_current_to_baseline_and_returns_the_sha_of_the_bytes() {
    let (_dir, _profiles, store) = store_in("bless-ok");
    let p = store.paths("qwen");
    let doc = profile_doc("qwen");
    std::fs::write(&p.current, &doc).unwrap();

    let blessing = store.bless("qwen").expect("bless");

    assert_eq!(blessing.path, p.baseline);
    assert_eq!(
        blessing.sha,
        sha256_hex(&doc),
        "the sha must be of the blessed BYTES, so `sha256sum` on the file verifies the journal row"
    );
    assert_eq!(std::fs::read_to_string(&p.baseline).unwrap(), doc);
    assert!(p.current.exists(), "bless copies, it does not move");
}

#[test]
fn bless_without_a_current_profile_is_a_named_error() {
    let (_dir, _profiles, store) = store_in("bless-absent");
    let p = store.paths("qwen");

    match store.bless("qwen") {
        Err(DriftError::NoCurrentProfile { model, path }) => {
            assert_eq!(model, "qwen");
            assert_eq!(path, p.current);
        }
        other => panic!("expected NoCurrentProfile, got {other:?}"),
    }
    assert!(!p.baseline.exists(), "a failed bless writes no baseline");
}

// ---------------------------------------------------------------------------
// Transient retention
// ---------------------------------------------------------------------------

#[test]
fn retain_transient_moves_the_file_to_a_content_addressed_name() {
    let (dir, _profiles, store) = store_in("transient-name");
    let doc = profile_doc("qwen");
    let src = dir.join("confirm-probe.json");
    std::fs::write(&src, &doc).unwrap();

    let kept = store.retain_transient("qwen", &src).expect("retain");

    assert_eq!(
        kept.retained.file_name().unwrap().to_str().unwrap(),
        format!("qwen.transient-{}.json", &sha256_hex(&doc)[..8])
    );
    assert!(kept.dropped.is_empty());
    assert!(!src.exists(), "the confirm document is moved, not copied");
    assert_eq!(std::fs::read_to_string(&kept.retained).unwrap(), doc);
}

/// Spec §5's bound: the latest N=4 transients, oldest dropped and returned so
/// the caller can journal the drop. Bounded *per model* — another model's
/// transients are none of this model's business.
#[test]
fn a_fifth_transient_drops_the_oldest_and_returns_its_path() {
    let (dir, _profiles, store) = store_in("transient-bound");

    let mut retained = Vec::new();
    for i in 0..MAX_TRANSIENTS {
        let src = dir.join(format!("confirm-{i}.json"));
        std::fs::write(&src, profile_doc(&format!("qwen-{i}"))).unwrap();
        let kept = store.retain_transient("qwen", &src).expect("retain");
        assert!(kept.dropped.is_empty(), "under the bound, nothing drops");
        retained.push(kept.retained);
    }

    // Deterministic, strictly increasing mtimes in the past, so "oldest" is a
    // fact about the files rather than about how fast the test ran.
    let base = SystemTime::now() - Duration::from_secs(3600);
    for (i, p) in retained.iter().enumerate() {
        set_mtime(p, base + Duration::from_secs(i as u64));
    }

    // A different model, at the bound's edge, must be untouched by qwen's prune.
    let other_src = dir.join("other-confirm.json");
    std::fs::write(&other_src, profile_doc("llama")).unwrap();
    let other = store.retain_transient("llama", &other_src).expect("retain");

    let src = dir.join("confirm-overflow.json");
    std::fs::write(&src, profile_doc("qwen-overflow")).unwrap();
    let kept = store.retain_transient("qwen", &src).expect("retain");

    assert_eq!(
        kept.dropped,
        vec![retained[0].clone()],
        "the oldest by mtime drops, and its path comes back for the journal"
    );
    assert!(!retained[0].exists());
    for p in &retained[1..] {
        assert!(p.exists(), "{} must survive", p.display());
    }
    assert!(kept.retained.exists());
    assert!(
        other.retained.exists(),
        "another model's transient is not qwen's to drop"
    );
}

/// The equal-mtime branch of the prune order, which the test above never
/// reaches because it stamps strictly increasing mtimes — and which a real
/// confirm loop hits routinely, since several files written inside one
/// filesystem tick share a timestamp exactly. With no tiebreak the drop would
/// be whatever order `read_dir` happened to hand back; this pins that ties
/// resolve to the lexicographically-first name. Arbitrary (the name is a
/// content hash) but *stable*, which is the property that makes a journaled
/// drop reproducible.
#[test]
fn transients_stamped_in_one_tick_drop_the_lexicographically_first_name() {
    let (dir, _profiles, store) = store_in("transient-tie");
    // One instant, shared by all five. `rename` preserves the inode, so the
    // stamp set on the source survives the move into the store — which is
    // what makes the tie a real tie rather than five "now"s microseconds
    // apart.
    let stamp = SystemTime::now() - Duration::from_secs(3600);

    let mut retained = Vec::new();
    for i in 0..MAX_TRANSIENTS {
        let src = dir.join(format!("tie-src-{i}.json"));
        std::fs::write(&src, profile_doc(&format!("tie-{i}"))).unwrap();
        set_mtime(&src, stamp);
        let kept = store.retain_transient("qwen", &src).expect("retain");
        assert!(kept.dropped.is_empty());
        retained.push(kept.retained);
    }

    let src = dir.join(format!("tie-src-{MAX_TRANSIENTS}.json"));
    std::fs::write(&src, profile_doc(&format!("tie-{MAX_TRANSIENTS}"))).unwrap();
    set_mtime(&src, stamp);
    let kept = store.retain_transient("qwen", &src).expect("retain");

    let mut all = retained.clone();
    all.push(kept.retained.clone());

    // Every one of the five really did share the tick — otherwise this test
    // would silently degrade into the mtime-ordered case above and stop
    // pinning the tiebreak at all.
    for p in all.iter().filter(|p| p.exists()) {
        assert_eq!(mtime(p), stamp, "{} lost the shared stamp", p.display());
    }

    let expected = all
        .iter()
        .min_by_key(|p| p.file_name().expect("transient has a name").to_os_string())
        .expect("five transients")
        .clone();
    assert_ne!(
        expected, kept.retained,
        "fixture precondition: the newly retained file must not be the one that drops, \
         so this test says nothing about the retained-then-pruned case"
    );

    assert_eq!(
        kept.dropped,
        vec![expected.clone()],
        "on an mtime tie the lexicographically-first name drops"
    );
    assert!(!expected.exists());
    for p in all.iter().filter(|p| **p != expected) {
        assert!(p.exists(), "{} must survive", p.display());
    }
}

// ---------------------------------------------------------------------------
// The gate — spec §3 (instrument precheck) and §4 (diff subprocess)
// ---------------------------------------------------------------------------

/// Real committed assay documents; provenance for all four is tabulated in
/// `profile_v8_fixture_test.rs`, which also pins the identity claims made
/// here.
///
/// - `V8_QWEN15B` / `V8_QWEN15B_DRYRUN` — **one model, one instrument, two
///   documents** (the 2026-08 campaign row and the dry-run row it superseded,
///   measured ~26 minutes apart). This is what a drift comparison actually
///   compares, so it is what the happy paths below use.
/// - `V8_QWEN3_8B` — same instrument, a **different model**: a crossed pair,
///   used only to pin the refusal.
/// - `V4_QWEN3_8B` — same model as `V8_QWEN3_8B`, the pre-upgrade instrument
///   (`0.5.0` / schema 4) the first post-upgrade boot will actually meet.
const V8_QWEN15B: &str = include_str!("fixtures/profile-v8-qwen15b.json");
const V8_QWEN15B_DRYRUN: &str = include_str!("fixtures/profile-v8-qwen15b-dryrun.json");
const V8_QWEN3_8B: &str = include_str!("fixtures/profile-v8-qwen3-8b.json");
const V4_QWEN3_8B: &str = include_str!("fixtures/profile-v4-qwen3-8b.json");

/// Every `(program, argv)` a gate spawned, in order. `Rc`/`RefCell` because a
/// [`bloomery_daemon::post::CommandRunner`] is deliberately not `Send` and
/// these tests are single-threaded.
type Calls = std::rc::Rc<std::cell::RefCell<Vec<(String, Vec<String>)>>>;

/// A wait status carrying exit code `code` — the encoding `waitpid` returns.
fn exited(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code << 8)
}

/// A wait status for a child killed by `sig`: no exit code at all, which is
/// the case `exit_code: None` exists for.
fn signalled(sig: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(sig)
}

/// A gate whose subprocess is scripted: it records every spawn and answers
/// with `status`. The recording is the point — several tests below assert the
/// diff was *never* spawned, which no assertion on the returned outcome could
/// establish.
fn gate_answering(status: std::process::ExitStatus) -> (DriftGate, Calls) {
    let calls: Calls = Calls::default();
    let sink = calls.clone();
    let gate = DriftGate::with_runner(Box::new(move |program: &str, args: &[String]| {
        sink.borrow_mut().push((program.to_string(), args.to_vec()));
        Ok(std::process::Output {
            status,
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }));
    (gate, calls)
}

/// A reference/current pair on disk. `None` writes no file at all, which is
/// how "first boot ever" and "baseline never blessed" actually look.
fn pair(tag: &str, reference: Option<&str>, current: Option<&str>) -> (PathBuf, PathBuf) {
    let dir = scratch(tag);
    let r = dir.join("reference.json");
    let c = dir.join("current.json");
    if let Some(doc) = reference {
        std::fs::write(&r, doc).unwrap();
    }
    if let Some(doc) = current {
        std::fs::write(&c, doc).unwrap();
    }
    (r, c)
}

fn sha_of(path: &Path) -> Option<String> {
    Some(sha256_hex_bytes(&std::fs::read(path).unwrap()))
}

/// Spec §4's invocation, to the letter. A value rather than a side effect of
/// spawning, so the contract with assay is readable in one line.
#[test]
fn diff_argv_is_the_documented_invocation() {
    assert_eq!(
        diff_argv(
            Path::new("/p/qwen.previous.json"),
            Path::new("/p/qwen.json")
        ),
        vec![
            "-m",
            "assay",
            "diff",
            "/p/qwen.previous.json",
            "/p/qwen.json",
            "--gate"
        ]
    );
}

/// A comparable pair really does spawn that invocation — and exactly once.
#[test]
fn a_comparable_pair_spawns_exactly_the_documented_diff() {
    let (r, c) = pair("gate-argv", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    assert_eq!(reading.outcome, GateOutcome::WithinNoise);
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1, "one comparison spawns one diff");
    // `config::default_python()`, which `with_runner` borrows so the two
    // spellings of "how this daemon runs python" cannot diverge.
    assert_eq!(calls[0].0, "python3");
    assert_eq!(calls[0].1, diff_argv(&r, &c));
}

#[test]
fn exit_zero_is_within_noise() {
    let (r, c) = pair("gate-exit0", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    assert_eq!(reading.outcome, GateOutcome::WithinNoise);
    assert_eq!(reading.exit_code, Some(0));
}

#[test]
fn exit_one_is_drift() {
    let (r, c) = pair("gate-exit1", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(1));

    let reading = gate.compare(&r, &c);

    assert_eq!(
        reading.outcome,
        GateOutcome::Drift,
        "exit 1 is a drift reading; whether it alarms is the confirm stage's call"
    );
    assert_eq!(reading.exit_code, Some(1));
}

/// Exit 2 is diff's own refusal to compare (one-sided tier marking and
/// friends). It is never a pass: the whole family of silent-pass bugs this
/// gate exists to refuse starts by folding a refusal into `WithinNoise`.
#[test]
fn exit_two_is_not_comparable_and_never_a_pass() {
    let (r, c) = pair("gate-exit2", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(2));

    let reading = gate.compare(&r, &c);

    assert_eq!(reading.outcome, GateOutcome::NotComparable { exit: 2 });
    assert_eq!(reading.exit_code, Some(2));
}

/// assay documents 0, 1 and 2 for `diff --gate`. Anything else is a tool this
/// daemon does not understand, so it is infrastructure — the one honest
/// reading. The code is still recorded: it exists, so `None` would be a lie.
#[test]
fn an_undocumented_exit_code_is_infrastructure_not_a_verdict() {
    let (r, c) = pair("gate-exit7", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(7));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Infra { detail } => assert!(
            detail.contains("undocumented exit 7"),
            "the unknown code must be named: {detail:?}"
        ),
        other => panic!("expected Infra for an undocumented exit, got {other:?}"),
    }
    assert_eq!(reading.exit_code, Some(7));
}

/// A signal-killed diff has no exit code at all. `-1` would look like a code;
/// `None` is what actually happened (None-vs-zero, applied to exit status).
#[test]
fn a_signal_killed_diff_is_infra_with_no_exit_code() {
    let (r, c) = pair("gate-signal", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(signalled(9));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Infra { detail } => assert!(
            detail.contains("signal"),
            "the kill must be named: {detail:?}"
        ),
        other => panic!("expected Infra for a signal-killed diff, got {other:?}"),
    }
    assert_eq!(reading.exit_code, None);
}

#[test]
fn a_spawn_failure_is_infra_naming_the_command() {
    let (r, c) = pair("gate-spawn-fail", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let gate = DriftGate::with_runner(Box::new(|_program, _args| {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file or directory",
        ))
    }));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Infra { detail } => {
            assert!(
                detail.contains("assay") && detail.contains("diff"),
                "the failed command must be named: {detail:?}"
            );
            assert!(
                detail.contains("no such file"),
                "the OS's own words must survive: {detail:?}"
            );
        }
        other => panic!("expected Infra for a spawn failure, got {other:?}"),
    }
    assert_eq!(reading.exit_code, None);
    // Both documents were read before the spawn was attempted, so both
    // digests are known even though the diff never ran.
    assert_eq!(reading.reference_sha, sha_of(&r));
    assert_eq!(reading.current_sha, sha_of(&c));
}

/// Two documents about different models measure nothing in common, so there
/// is no comparison to run — the same rule `post::PostRunner::probe` already
/// applies to a single document ("attaching it would credit one model with
/// another's measurements"), applied to the pair. Without this the crossed
/// pair diffs cleanly and `drift_event` stamps the caller's model name over
/// another model's document: a verdict about a model nobody measured.
///
/// Checked *before* the instrument precheck: on a crossed pair the instrument
/// answer is noise either way (these two happen to share an instrument, so it
/// would read `Comparable` and spawn).
#[test]
fn a_crossed_pair_of_models_is_unmeasured_and_never_spawns() {
    let (r, c) = pair("gate-crossed", Some(V8_QWEN3_8B), Some(V8_QWEN15B));
    let (gate, calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Unmeasured { reason } => assert!(
            reason.contains("qwen3:8b") && reason.contains("qwen2.5-coder:1.5b-instruct-q8_0"),
            "both models must be named so the operator can see which pair crossed: {reason:?}"
        ),
        other => panic!("expected Unmeasured for a crossed pair, got {other:?}"),
    }
    assert!(
        calls.borrow().is_empty(),
        "a crossed pair must never reach the diff, got {:?}",
        calls.borrow()
    );
    assert_eq!(reading.exit_code, None);
    // Both documents were read, so both digests stand — the row stays
    // verifiable even though no comparison was made.
    assert_eq!(reading.reference_sha, sha_of(&r));
    assert_eq!(reading.current_sha, sha_of(&c));
}

/// Spec §3, and the load-bearing ordering claim of this whole task: the
/// precheck reads the two documents' own version fields and refuses *before*
/// any subprocess exists. A gate that spawned first and prechecked after would
/// return the same outcome here — only the never-spawned assertion catches it.
#[test]
fn a_changed_instrument_is_named_before_the_diff_is_ever_spawned() {
    let (r, c) = pair("gate-instrument", Some(V4_QWEN3_8B), Some(V8_QWEN3_8B));
    let (gate, calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    assert_eq!(
        reading.outcome,
        GateOutcome::InstrumentChanged {
            reference: "0.5.0/v4".to_string(),
            current: "0.9.0/v8".to_string(),
        }
    );
    assert!(
        calls.borrow().is_empty(),
        "the precheck must run BEFORE the diff is spawned, got {:?}",
        calls.borrow()
    );
    assert_eq!(reading.exit_code, None, "no diff ran, so there is no code");
    assert_eq!(reading.reference_sha, sha_of(&r));
    assert_eq!(reading.current_sha, sha_of(&c));
}

/// First boot ever, or a baseline nobody blessed. `unmeasured` by name — never
/// a pass, and never a spawn (there is nothing to hand assay).
#[test]
fn an_absent_reference_is_unmeasured_and_never_spawns() {
    let (r, c) = pair("gate-absent-ref", None, Some(V8_QWEN3_8B));
    let (gate, calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Unmeasured { reason } => {
            assert!(
                reason.contains("reference") && reason.contains(&r.display().to_string()),
                "the missing side and its path must be named: {reason:?}"
            );
        }
        other => panic!("expected Unmeasured for an absent reference, got {other:?}"),
    }
    assert!(
        calls.borrow().is_empty(),
        "an absent reference must never spawn a diff"
    );
    assert_eq!(reading.exit_code, None);
    assert_eq!(
        reading.reference_sha, None,
        "a file that was never read has no digest — None, not a digest of nothing"
    );
    assert_eq!(reading.current_sha, sha_of(&c));
}

/// A reference whose bytes exist but are not a profile is also unmeasured —
/// and its digest is still recorded, because those bytes were read and an
/// operator may want to check exactly which document failed to parse.
#[test]
fn an_unparseable_reference_is_unmeasured_with_its_bytes_still_named() {
    let (r, c) = pair("gate-bad-ref", Some("{ truncated json"), Some(V8_QWEN3_8B));
    let (gate, calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Unmeasured { reason } => {
            assert!(
                reason.contains(&r.display().to_string()),
                "the unparseable document must be named: {reason:?}"
            );
            assert!(!reason.is_empty(), "the refusal must say why");
        }
        other => panic!("expected Unmeasured for an unparseable reference, got {other:?}"),
    }
    assert!(calls.borrow().is_empty());
    assert_eq!(
        reading.reference_sha,
        sha_of(&r),
        "the bytes were read, so their digest is known"
    );
}

/// The current side too: a boot where POST failed has nothing to compare, and
/// that is `unmeasured`, not a clean bill of health.
#[test]
fn an_absent_current_is_unmeasured_too() {
    let (r, c) = pair("gate-absent-cur", Some(V8_QWEN3_8B), None);
    let (gate, calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Unmeasured { reason } => assert!(
            reason.contains("current") && reason.contains(&c.display().to_string()),
            "the missing side and its path must be named: {reason:?}"
        ),
        other => panic!("expected Unmeasured for an absent current, got {other:?}"),
    }
    assert!(calls.borrow().is_empty());
    assert_eq!(reading.current_sha, None);
    assert_eq!(reading.reference_sha, sha_of(&r));
}

/// The controller ruling: a drift row's file claims are byte-verifiable the
/// way a `Blessed` row's are. The digests are of each file's **bytes**, taken
/// at comparison time — so `sha256sum` on the path in the row checks the row.
#[test]
fn the_digests_are_of_the_files_bytes_so_the_row_is_checkable() {
    let (r, c) = pair("gate-sha", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    assert_eq!(reading.reference_sha, sha_of(&r));
    assert_eq!(reading.current_sha, sha_of(&c));
    assert_ne!(
        reading.reference_sha, reading.current_sha,
        "two different documents must not share a digest"
    );
    // A digest of the *path string* would also be 64 hex characters and would
    // also differ between the two sides — only comparing against the bytes
    // tells the two apart.
    assert_ne!(
        reading.reference_sha,
        Some(sha256_hex(&r.display().to_string())),
        "the digest must be of the document's bytes, not of its path"
    );
    assert_eq!(reading.reference_sha.as_ref().unwrap().len(), 64);
}

/// A wedged diff is bounded and named, not a verdict. Drives the *real*
/// bounded-spawn layer (the injected runner replaces the whole subprocess and
/// so cannot exercise it) against a child that outlives its cap.
#[test]
fn a_wedged_diff_is_infra_not_a_verdict() {
    let (r, c) = pair("gate-wedged", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let gate = DriftGate::with_runner(Box::new(|_program, _args| {
        bloomery_daemon::post::run_bounded_for_test(
            "/bin/sh",
            &["-c".to_string(), "sleep 5".to_string()],
            Duration::from_millis(300),
        )
    }));

    let started = Instant::now();
    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Infra { detail } => assert!(
            detail.contains("timed out"),
            "the expiry must be named: {detail:?}"
        ),
        other => panic!("expected Infra for a wedged diff, got {other:?}"),
    }
    assert_eq!(reading.exit_code, None, "a killed diff reported no code");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the cap must bound the wait, not the child"
    );
}

/// `assay diff` reads two JSON documents; it drives no model, so it is not
/// bounded by the probe cap an operator raises for slow, partially-offloaded
/// models. Pinned against the shipped default rather than against a literal,
/// so the relationship is what is asserted.
#[test]
fn the_diff_cap_is_its_own_constant_not_the_probe_cap() {
    let gate = DriftGate::new("python3".to_string());
    assert_eq!(gate.timeout(), Duration::from_secs(DIFF_TIMEOUT_SECS));

    let dir = scratch("gate-timeout-config");
    let cfg_path = dir.join("bloomery.toml");
    std::fs::write(
        &cfg_path,
        r#"
port = 9000
data_dir = "/tmp/bloomery-drift-gate-timeout"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = true, python = "python3" }

[models]
qwen = "/models/qwen.gguf"
"#,
    )
    .unwrap();
    let config = load_config(&cfg_path).expect("config");
    assert!(
        DIFF_TIMEOUT_SECS < config.assay.probe_timeout_secs,
        "the offline diff must not inherit the model-driving probe's cap \
         ({DIFF_TIMEOUT_SECS}s vs {}s)",
        config.assay.probe_timeout_secs
    );
}

// ---------------------------------------------------------------------------
// The journal row
// ---------------------------------------------------------------------------

/// Every field of the row comes from the reading the gate actually produced —
/// the paths it compared, the digests of the bytes at those paths, and the
/// code diff reported. Nothing is re-derived by the caller, so the row cannot
/// describe a different read than the comparison did.
#[test]
fn a_drift_row_carries_the_readings_paths_digests_and_exit_code() {
    let (r, c) = pair("gate-row", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(1));
    let reading = gate.compare(&r, &c);

    match drift_event("qwen3:8b", Comparison::Step, &reading) {
        Event::Drift {
            model,
            comparison,
            outcome,
            reference_path,
            current_path,
            exit_code,
            reference_sha,
            current_sha,
        } => {
            assert_eq!(model, "qwen3:8b");
            assert_eq!(comparison, "step");
            assert_eq!(outcome, "drift");
            assert_eq!(reference_path, r.display().to_string());
            assert_eq!(current_path, c.display().to_string());
            assert_eq!(exit_code, Some(1));
            assert_eq!(reference_sha, sha_of(&r));
            assert_eq!(current_sha, sha_of(&c));
        }
        other => panic!("expected a Drift row, got {other:?}"),
    }
}

/// The unmeasured row names its comparison and leaves every number absent —
/// no zero exit code, no digest for a file that was never read.
#[test]
fn an_unmeasured_row_names_the_comparison_and_leaves_the_numbers_absent() {
    let (r, c) = pair("gate-row-unmeasured", None, Some(V8_QWEN3_8B));
    let (gate, _calls) = gate_answering(exited(0));
    let reading = gate.compare(&r, &c);

    match drift_event("qwen3:8b", Comparison::Cumulative, &reading) {
        Event::Drift {
            comparison,
            outcome,
            exit_code,
            reference_sha,
            ..
        } => {
            assert_eq!(comparison, "cumulative");
            assert!(
                outcome.starts_with("unmeasured:") && outcome.contains(&r.display().to_string()),
                "the row must name the refusal and its cause: {outcome:?}"
            );
            assert_eq!(exit_code, None);
            assert_eq!(reference_sha, None);
        }
        other => panic!("expected a Drift row, got {other:?}"),
    }
}

/// Spec §3's row: both instrument identities in the outcome, so a replay can
/// say what moved without re-reading either document. Identity strings, not
/// transcribed measurements.
#[test]
fn an_instrument_changed_row_names_both_instruments() {
    let (r, c) = pair("gate-row-instrument", Some(V4_QWEN3_8B), Some(V8_QWEN3_8B));
    let (gate, _calls) = gate_answering(exited(0));
    let reading = gate.compare(&r, &c);

    match drift_event("qwen3:8b", Comparison::Cumulative, &reading) {
        Event::Drift { outcome, .. } => assert!(
            outcome.contains("instrument-changed")
                && outcome.contains("0.5.0/v4")
                && outcome.contains("0.9.0/v8"),
            "both sides' instrument identities belong in the row: {outcome:?}"
        ),
        other => panic!("expected a Drift row, got {other:?}"),
    }
}

/// The row survives a journal round trip with its `None`s intact — an absent
/// digest must not come back as an empty string, and an absent exit code must
/// not come back as 0.
#[test]
fn a_drift_row_round_trips_through_the_journal() {
    let dir = scratch("gate-row-journal");
    let (r, c) = pair("gate-row-journal-docs", None, Some(V8_QWEN3_8B));
    let (gate, _calls) = gate_answering(exited(0));
    let event = drift_event("qwen3:8b", Comparison::Step, &gate.compare(&r, &c));

    let jpath = dir.join("j.jsonl");
    let mut j = Journal::open(&jpath).unwrap();
    j.append(&event).unwrap();

    assert_eq!(replay(&jpath).unwrap(), vec![event]);
}

// ---------------------------------------------------------------------------
// POST wiring for the first test
// ---------------------------------------------------------------------------

fn qwen_like_meta() -> bloomery_core::gguf::GgufMeta {
    bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
    }
}

fn tier() -> Tier {
    Tier {
        name: "enthusiast-16gb".into(),
        emulated: false,
    }
}

/// A fake assay: writes a real profile document to whatever `--json` path it
/// was handed, exactly as assay does.
fn scripted_assay() -> PostRunner {
    PostRunner::with_runner(Box::new(move |_py, args| {
        let value_of = |flag: &str| -> String {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_default()
        };
        let model = value_of("--model");
        std::fs::write(value_of("--json"), profile_doc(&model)).unwrap();
        use std::os::unix::process::ExitStatusExt;
        Ok(std::process::Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }))
}
