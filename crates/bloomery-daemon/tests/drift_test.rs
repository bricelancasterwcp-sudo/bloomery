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
    diff_argv, drift_event, profile_file_name, Comparison, DriftError, DriftGate, DriftStatus,
    GateOutcome, ModelDrift, ProfileStore, Rotation, DIFF_TIMEOUT_SECS, MAX_TRANSIENTS,
};
use bloomery_daemon::pager::{Pager, PagerError};
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
    profile_doc_ceiling(model, 2048)
}

/// [`profile_doc`] with the ceiling as a knob, so a test can put two
/// *different* documents on disk that are still the same model measured by the
/// same instrument — which is what a drift comparison actually meets, and what
/// makes "which document is the reference" a checkable claim rather than a
/// tautology.
fn profile_doc_ceiling(model: &str, max_verified: u32) -> String {
    format!(
        r#"{{"assay_profile_version":3,"probe_version":"0.4.1","model":{{"name":"{model}"}},"ceiling":{{"max_verified":{max_verified}}},"verdicts":{{}}}}"#
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
///
/// It is also the **only** test that reaches the drift watch through
/// production `run_post` rather than the `run_post_with_gate` seam every
/// orchestration test below uses, so it pins the delegation itself: a
/// `run_post` that skipped the watch would still pass every scripted-gate test
/// and fail here. Staying python-free costs nothing — this is a first boot,
/// both references are absent, and design §3's precheck refuses an unmeasurable
/// comparison *before* any subprocess exists, so the real `DriftGate` inside
/// `run_post` never spawns.
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

    // The drift watch really is wired into production `run_post`: this first
    // boot blessed its own profile as the baseline, on disk and in the record.
    let baseline = store.paths("qwen").baseline;
    assert_eq!(
        std::fs::read_to_string(&baseline)
            .expect("the real run_post drives the drift watch, which blesses a first profile"),
        profile_doc("qwen")
    );
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Blessed { model, provenance, sha, .. }
                if model == "qwen"
                    && provenance == "auto-first-profile"
                    && *sha == sha256_hex_bytes(profile_doc("qwen").as_bytes()))),
        "expected a Blessed row from the real boot path, got {events:?}"
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

/// Exit 3 is assay ≥ 0.10's incomplete comparison: a cell measured on exactly
/// one side, outranking a measured drift (its precedence is 2 > 3 > 1 > 0).
/// It is a documented verdict, not an undocumented code — and it is never a
/// pass, because the cells it could not score may hide exactly the move the
/// gate exists to catch.
#[test]
fn exit_three_is_incomplete_and_never_a_pass() {
    let (r, c) = pair("gate-exit3", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(3));

    let reading = gate.compare(&r, &c);

    assert_eq!(reading.outcome, GateOutcome::Incomplete { exit: 3 });
    assert_eq!(reading.exit_code, Some(3));
    assert_eq!(reading.outcome.journal_outcome(), "incomplete");
}

/// assay documents 0, 1, 2 and (since 0.10) 3 for `diff --gate`. Anything
/// else is a tool this daemon does not understand, so it is infrastructure —
/// the one honest reading. The code is still recorded: it exists, so `None`
/// would be a lie.
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
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
        value_length: None,
        recurrent_state_bytes: 0,
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

// ---------------------------------------------------------------------------
// Confirm-then-alarm, wired into the boot (spec §2, §4, §5)
//
// Every test below drives the *real* `run_post` orchestration against a fake
// assay and a scripted gate: the probe count, the journal rows and the
// rendered status all come from the shipping code path, not from a
// re-implementation of it in the test.
// ---------------------------------------------------------------------------

/// Where a fake assay was asked to write, once per probe, in order. The
/// *length* is the load-bearing part: confirm-then-alarm is a claim about how
/// many times a model is probed in one boot.
type Probes = std::rc::Rc<std::cell::RefCell<Vec<PathBuf>>>;

fn output(status: std::process::ExitStatus, stderr: &str) -> std::process::Output {
    std::process::Output {
        status,
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

fn value_of(args: &[String], flag: &str) -> String {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default()
}

/// A fake assay whose Nth probe follows `script[N]` (the last entry repeats):
/// `Ok(doc)` writes that document to the `--json` path it was handed and exits
/// 0; `Err(code)` exits `code` having written nothing, exactly as a failing
/// probe does.
fn scripted_probes(script: Vec<Result<String, i32>>) -> (PostRunner, Probes) {
    let seen: Probes = Probes::default();
    let sink = seen.clone();
    let runner = PostRunner::with_runner(Box::new(move |_py, args: &[String]| {
        let out = PathBuf::from(value_of(args, "--json"));
        let model = value_of(args, "--model");
        let step = script[sink.borrow().len().min(script.len() - 1)].clone();
        sink.borrow_mut().push(out.clone());
        match step {
            Ok(doc) => {
                std::fs::write(&out, doc).unwrap();
                Ok(output(exited(0), ""))
            }
            Err(code) => Ok(output(exited(code), &format!("cannot reach model {model}"))),
        }
    }));
    (runner, seen)
}

/// A gate that decides each comparison from the pair of paths it was handed,
/// recording every spawn. The decision function is what a test uses to say
/// "the step comparison drifts but the cumulative one does not", and
/// "…and the confirm's re-diff agrees".
fn gate_deciding(
    decide: impl Fn(&str, &str) -> std::process::ExitStatus + 'static,
) -> (DriftGate, Calls) {
    let calls: Calls = Calls::default();
    let sink = calls.clone();
    let gate = DriftGate::with_runner(Box::new(move |program: &str, args: &[String]| {
        sink.borrow_mut().push((program.to_string(), args.to_vec()));
        let empty = String::new();
        let reference = args.get(3).unwrap_or(&empty).clone();
        let current = args.get(4).unwrap_or(&empty).clone();
        Ok(output(decide(&reference, &current), ""))
    }));
    (gate, calls)
}

/// True for a confirm run's document: retention names it by content, and that
/// name is how a test (and an operator reading the journal) tells the confirm
/// re-diff from the first reading.
fn is_transient(path: &str) -> bool {
    path.contains(".transient-")
}

/// One model's whole boot: a real `Pager` with a real journal, the profiles
/// directory `main.rs` creates, and `qwen` registered but unprofiled — the
/// state `run_post` actually meets.
struct Boot {
    profiles: PathBuf,
    jpath: PathBuf,
    pager: std::sync::Mutex<Pager<FakeSubstrate>>,
    model: String,
}

fn boot(tag: &str) -> Boot {
    boot_for(tag, "qwen")
}

/// [`boot`], but registering `model` instead of the hardcoded `"qwen"`.
///
/// Exists for exactly one thing `boot` cannot do: drive the real committed
/// fixtures (`fixtures/profile-v{4,8}-qwen3-8b.json`), whose own
/// `model.name` is `"qwen3:8b"` — `PostRunner::probe` refuses a document
/// whose `model.name` does not match the model it was asked to probe, so a
/// test reaching for those bytes as committed needs the pager to register
/// them under their own name, not a relabelled `"qwen"`.
fn boot_for(tag: &str, model: &str) -> Boot {
    let (dir, profiles, _store) = store_in(tag);
    let jpath = dir.join("j.jsonl");
    let mut pager = Pager::new(
        FakeSubstrate::new(),
        Journal::open(&jpath).expect("journal"),
        ImageStore::new(&dir.join("img")).expect("image store"),
        Box::new(|| Some(10u64.pow(9))),
    );
    let gguf = dir.join(format!("{model}.gguf"));
    std::fs::write(&gguf, b"weights").unwrap();
    pager
        .register_model(model, &gguf, qwen_like_meta(), None)
        .unwrap();
    pager.set_posting(true);
    Boot {
        profiles,
        jpath,
        pager: std::sync::Mutex::new(pager),
        model: model.to_string(),
    }
}

impl Boot {
    /// Writes a document into the profiles directory as if an earlier boot (or
    /// an operator's blessing) had left it there.
    fn seed(&self, name: &str, doc: &str) {
        std::fs::write(self.profiles.join(name), doc).unwrap();
    }

    fn run(&self, runner: &PostRunner, gate: &DriftGate) {
        bloomery_daemon::post::run_post_with_gate(
            &self.pager,
            runner,
            std::slice::from_ref(&self.model),
            8181,
            &tier(),
            &self.profiles,
            gate,
        )
        .expect("POST records its result");
    }

    fn events(&self) -> Vec<Event> {
        replay(&self.jpath).unwrap()
    }

    /// Every drift row as `(comparison, outcome, current_path)` — the three
    /// fields the orchestration's claims are about.
    fn drift_rows(&self) -> Vec<(String, String, String)> {
        self.events()
            .iter()
            .filter_map(|e| match e {
                Event::Drift {
                    comparison,
                    outcome,
                    current_path,
                    ..
                } => Some((comparison.clone(), outcome.clone(), current_path.clone())),
                _ => None,
            })
            .collect()
    }

    fn drift(&self) -> Option<ModelDrift> {
        self.pager
            .lock()
            .unwrap()
            .status()
            .models
            .into_iter()
            .find(|m| m.name == self.model)
            .expect("model is registered")
            .drift
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.profiles.join(name)).unwrap_or_else(|e| {
            panic!("expected {name} in the profiles directory: {e}");
        })
    }

    fn transients(&self) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(&self.profiles)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| is_transient(&p.display().to_string()))
            .collect();
        found.sort();
        found
    }
}

fn sha8(doc: &str) -> String {
    sha256_hex_bytes(doc.as_bytes())[..8].to_string()
}

/// A boot where nothing moved: both comparisons run, both read within noise,
/// the model is probed exactly once, and a baseline that already exists is not
/// re-blessed behind the operator's back.
#[test]
fn a_clean_boot_reads_within_noise_on_both_comparisons_and_probes_once() {
    let b = boot("watch-clean");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024)); // last boot's
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|_r, _c| exited(0));

    b.run(&runner, &gate);

    assert_eq!(
        probes.borrow().len(),
        1,
        "a boot with no drift reading probes once, never speculatively twice"
    );
    assert_eq!(
        b.drift(),
        Some(ModelDrift {
            step: DriftStatus::WithinNoise,
            cumulative: DriftStatus::WithinNoise,
        })
    );
    assert_eq!(
        b.drift_rows()
            .iter()
            .map(|(c, o, _)| (c.as_str(), o.as_str()))
            .collect::<Vec<_>>(),
        vec![("step", "within-noise"), ("cumulative", "within-noise")],
        "exactly one row per comparison, each naming its own verdict"
    );
    assert!(
        !b.events()
            .iter()
            .any(|e| matches!(e, Event::Blessed { .. })),
        "a baseline that already exists is never re-blessed by the daemon"
    );
}

/// A FIRST diff exiting 3 (assay ≥ 0.10's incomplete comparison) settles
/// without a confirm: spec §4 reserves the confirm for the Drift hypothesis,
/// and an incomplete comparison asserts no drift to reproduce. The row names
/// the settled verdict, and it is never a pass.
#[test]
fn a_first_diff_exiting_three_settles_incomplete_with_no_confirm() {
    let b = boot("watch-incomplete");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024)); // last boot's
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|reference, _current| {
        if reference.ends_with(".previous.json") {
            exited(3)
        } else {
            exited(0)
        }
    });

    b.run(&runner, &gate);

    assert_eq!(
        probes.borrow().len(),
        1,
        "an incomplete first reading earns no confirm probe beyond the boot's own POST"
    );
    assert_eq!(
        b.drift(),
        Some(ModelDrift {
            step: DriftStatus::Incomplete,
            cumulative: DriftStatus::WithinNoise,
        })
    );
    assert_eq!(
        b.drift_rows()
            .iter()
            .map(|(c, o, _)| (c.as_str(), o.as_str()))
            .collect::<Vec<_>>(),
        vec![("step", "incomplete"), ("cumulative", "within-noise")],
        "one row per comparison; the step row spells the settled verdict, not a pass"
    );
    assert!(
        !b.events()
            .iter()
            .any(|e| matches!(e, Event::Blessed { .. })),
        "a baseline that already exists is never re-blessed by the daemon"
    );
}

/// The rotation law (spec §5), pinned behaviourally: the step comparison's
/// reference is LAST boot's document, because rotation runs before POST's
/// delete-before-probe. Rotating after the probe would leave the step
/// comparison diffing this boot's document against itself — a gate that can
/// only ever read within-noise.
#[test]
fn the_step_reference_is_last_boots_document_rotated_before_this_boots_probe() {
    let b = boot("watch-rotate-first");
    let last_boot = profile_doc_ceiling("qwen", 1024);
    b.seed("qwen.json", &last_boot);
    let (runner, _probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|_r, _c| exited(0));

    b.run(&runner, &gate);

    assert_eq!(
        b.read("qwen.previous.json"),
        last_boot,
        "previous must hold LAST boot's measurement"
    );
    assert_eq!(
        b.read("qwen.json"),
        profile_doc("qwen"),
        "current must hold THIS boot's measurement"
    );
    let step = b
        .events()
        .into_iter()
        .find(|e| matches!(e, Event::Drift { comparison, .. } if comparison == "step"))
        .expect("a step row");
    match step {
        Event::Drift {
            reference_sha,
            current_sha,
            ..
        } => {
            assert_eq!(
                reference_sha,
                Some(sha256_hex_bytes(last_boot.as_bytes())),
                "the step row's reference digest is last boot's document"
            );
            assert_ne!(
                reference_sha, current_sha,
                "a comparison of this boot's document against itself measures nothing"
            );
        }
        other => panic!("expected a Drift row, got {other:?}"),
    }
}

/// Spec §4's confirm-then-alarm: a drift reading is a hypothesis, and the
/// confirm re-probe tests it. Exactly two probes — the boot's, and the one
/// confirm — and the alarm is only raised because the second diff agreed.
#[test]
fn a_step_drift_that_reproduces_is_confirmed_after_exactly_one_re_probe() {
    let b = boot("watch-confirmed");
    let last_boot = profile_doc_ceiling("qwen", 1024);
    b.seed("qwen.json", &last_boot);
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    // The step reference drifts every time it is asked; the cumulative one
    // does not — so exactly one comparison has a hypothesis to confirm.
    let (gate, _calls) = gate_deciding(|reference, _current| {
        if reference.ends_with(".previous.json") {
            exited(1)
        } else {
            exited(0)
        }
    });

    b.run(&runner, &gate);

    assert_eq!(
        probes.borrow().len(),
        2,
        "one boot probe plus exactly one confirm — never zero, never a retry loop"
    );
    assert_eq!(
        b.drift(),
        Some(ModelDrift {
            step: DriftStatus::Confirmed {
                reference: sha8(&last_boot),
            },
            cumulative: DriftStatus::WithinNoise,
        })
    );
    let rows = b.drift_rows();
    assert_eq!(
        rows.iter()
            .map(|(c, o, _)| (c.as_str(), o.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("step", "drift"),
            ("step", "confirmed"),
            ("cumulative", "within-noise")
        ],
        "the first reading journals what the gate said; its confirm journals the verdict that \
         settled it — `confirmed`, never the raw `drift` word again: {rows:?}"
    );
    assert!(
        is_transient(&rows[1].2),
        "the confirm's row must name the fresh document it compared, got {:?}",
        rows[1].2
    );
    assert!(
        std::path::Path::new(&rows[1].2).exists(),
        "the confirm document the row names must be on disk to be checkable"
    );
    assert_eq!(
        b.read("qwen.json"),
        profile_doc("qwen"),
        "the confirm probe never overwrites this boot's measurement"
    );
}

/// Spec §4's second outcome, and assay's founding finding: the serving state
/// moved between two probes of one boot. That is a finding of its own, not an
/// alarm — and the document that failed to reproduce is kept beside the row.
#[test]
fn a_step_drift_that_does_not_reproduce_is_transient_and_its_document_is_kept() {
    let b = boot("watch-transient");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024));
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|reference, current| {
        if reference.ends_with(".previous.json") && !is_transient(current) {
            exited(1)
        } else {
            exited(0)
        }
    });

    b.run(&runner, &gate);

    assert_eq!(probes.borrow().len(), 2);
    assert_eq!(
        b.drift().map(|d| d.step),
        Some(DriftStatus::Transient),
        "a reading that does not reproduce is transient, never confirmed"
    );
    let kept = b.transients();
    assert_eq!(kept.len(), 1, "the confirm document is retained: {kept:?}");
    assert_eq!(
        std::fs::read_to_string(&kept[0]).unwrap(),
        profile_doc("qwen")
    );
    let step_rows: Vec<(String, String, String)> = b
        .drift_rows()
        .into_iter()
        .filter(|(c, _, _)| c == "step")
        .collect();
    assert_eq!(
        step_rows.len(),
        2,
        "both the reading and its confirm are journaled"
    );
    assert_eq!(
        step_rows[1].1, "transient",
        "the confirm's row spells the finding — a transient is NOT the `within-noise` a clean \
         boot gets, and the two must never share a word: {step_rows:?}"
    );
}

/// Spec §4's wedged-confirm rule: when the confirm probe itself fails there is
/// no second reading, so the first one stands as `unconfirmed` — NAMED, and
/// never silently upgraded to `Confirmed`.
#[test]
fn a_confirm_probe_that_fails_leaves_the_reading_unconfirmed_and_never_upgrades_it() {
    let b = boot("watch-unconfirmed");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024));
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, probes) = scripted_probes(vec![Ok(profile_doc("qwen")), Err(4)]);
    let (gate, _calls) = gate_deciding(|reference, _current| {
        if reference.ends_with(".previous.json") {
            exited(1)
        } else {
            exited(0)
        }
    });

    b.run(&runner, &gate);

    assert_eq!(probes.borrow().len(), 2, "the confirm was attempted");
    match b.drift().map(|d| d.step) {
        Some(DriftStatus::Unconfirmed { reason }) => assert!(
            reason.contains("assay exited 4") && reason.contains("cannot reach model"),
            "the failure that prevented the confirm must be named: {reason:?}"
        ),
        other => panic!("expected Unconfirmed after a failed confirm probe, got {other:?}"),
    }
    assert_eq!(
        b.drift_rows()
            .iter()
            .filter(|(c, _, _)| c == "step")
            .count(),
        1,
        "a confirm that never produced a document journals no second comparison"
    );
    // …but it does not vanish either: the probe can burn the whole
    // `probe_timeout_secs` window and die, and spec §4 says a confirm that
    // could not be made journals as infrastructure. A status field is not a
    // record.
    assert!(
        b.events().iter().any(|e| matches!(e,
            Event::Degraded { reason }
                if reason.contains("confirm probe")
                    && reason.contains("qwen")
                    && reason.contains("step")
                    && reason.contains("assay exited 4"))),
        "the failed confirm must leave a durable row naming the model, the comparison and the \
         failure: {:?}",
        b.events()
    );
}

/// Spec §4's third outcome: the confirm's re-diff refusing to compare is
/// infrastructure-shaped, not a drift verdict — so the reading stays
/// unconfirmed, naming what the re-diff answered.
#[test]
fn a_confirm_re_diff_that_refuses_is_unconfirmed_naming_the_refusal() {
    let b = boot("watch-unconfirmed-refusal");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024));
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, _probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|reference, current| {
        if !reference.ends_with(".previous.json") {
            exited(0)
        } else if is_transient(current) {
            exited(2)
        } else {
            exited(1)
        }
    });

    b.run(&runner, &gate);

    match b.drift().map(|d| d.step) {
        Some(DriftStatus::Unconfirmed { reason }) => assert!(
            reason.contains("not-comparable"),
            "the re-diff's own answer must be named: {reason:?}"
        ),
        other => panic!("expected Unconfirmed for a re-diff that refused, got {other:?}"),
    }
    let step_rows: Vec<(String, String, String)> = b
        .drift_rows()
        .into_iter()
        .filter(|(c, _, _)| c == "step")
        .collect();
    assert_eq!(
        step_rows[1].1, "unconfirmed: not-comparable",
        "the confirm's row names both the verdict and what the re-diff answered: {step_rows:?}"
    );
}

/// The pinned ordering (spec §2 + the controller's ruling): the first profile
/// auto-blesses AFTER this boot's comparisons have run, so the cumulative
/// comparison on that boot honestly reads `unmeasured` — there was no baseline
/// when it was asked. Blessing first would hand the gate a baseline byte-identical
/// to the current document and manufacture a within-noise pass out of nothing.
#[test]
fn the_first_profile_auto_blesses_after_the_comparisons_so_cumulative_reads_unmeasured() {
    let b = boot("watch-auto-bless");
    let (runner, _probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|_r, _c| exited(0));

    b.run(&runner, &gate);

    match b.drift().map(|d| d.cumulative) {
        Some(DriftStatus::Unmeasured { reason }) => assert!(
            reason.contains("qwen.baseline.json"),
            "the missing reference must be named: {reason:?}"
        ),
        other => panic!("expected Unmeasured cumulative on the blessing boot, got {other:?}"),
    }
    assert_eq!(
        b.read("qwen.baseline.json"),
        profile_doc("qwen"),
        "the baseline is this boot's document, byte for byte"
    );
    let blessed = b
        .events()
        .into_iter()
        .find(|e| matches!(e, Event::Blessed { .. }))
        .expect("the first profile is blessed");
    match blessed {
        Event::Blessed {
            model,
            profile_path,
            sha,
            provenance,
        } => {
            assert_eq!(model, "qwen");
            assert!(profile_path.ends_with("qwen.baseline.json"));
            assert_eq!(sha, sha256_hex_bytes(profile_doc("qwen").as_bytes()));
            assert_eq!(
                provenance, "auto-first-profile",
                "the provenance of every baseline is explicit"
            );
        }
        other => panic!("expected a Blessed row, got {other:?}"),
    }
}

/// `ModelStatus.drift` is `None` when the drift watch never ran for that model
/// this boot — the same None-honesty `done_trust` has: absent is not clean.
#[test]
fn a_model_whose_post_failed_has_no_drift_reading_at_all() {
    let b = boot("watch-post-failed");
    let (runner, _probes) = scripted_probes(vec![Err(4)]);
    let (gate, calls) = gate_deciding(|_r, _c| exited(0));

    b.run(&runner, &gate);

    assert_eq!(
        b.drift(),
        None,
        "no measurement means no verdict — absent, never a clean one"
    );
    assert!(
        b.drift_rows().is_empty(),
        "a boot with no current document has nothing to compare"
    );
    assert!(
        calls.borrow().is_empty(),
        "no comparison is attempted at all"
    );
}

/// The rendered surface: both fields present under their own names, and a
/// model that was never compared renders `null` rather than a verdict.
#[test]
fn status_renders_the_drift_pair_and_null_when_it_never_ran() {
    let b = boot("watch-status-json");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024));
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, _probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|reference, current| {
        if reference.ends_with(".previous.json") && !is_transient(current) {
            exited(1)
        } else {
            exited(0)
        }
    });

    let unmeasured: serde_json::Value = {
        let p = b.pager.lock().unwrap();
        serde_json::to_value(p.status()).unwrap()
    };
    assert_eq!(
        unmeasured["models"][0]["drift"],
        serde_json::Value::Null,
        "before the watch runs, drift is null — absent, not clean"
    );

    b.run(&runner, &gate);

    let rendered: serde_json::Value = {
        let p = b.pager.lock().unwrap();
        serde_json::to_value(p.status()).unwrap()
    };
    let drift = &rendered["models"][0]["drift"];
    assert_eq!(drift["step"]["status"], "transient");
    assert_eq!(drift["cumulative"]["status"], "within-noise");
    assert_eq!(
        rendered["models"][0]["done_trust"],
        serde_json::Value::Null,
        "drift is its own field and says nothing about done_trust"
    );
}

/// Spec §5's rotation-on-successful-parse rule, from the boot's side: a
/// corrupt current document is never promoted to "the previous boot's
/// measurement", the older good reference survives, and the degradation of the
/// drift record is journaled — POST's delete-before-probe then reclaims the
/// bytes, so the row is what remains of them.
#[test]
fn an_unparseable_current_document_is_kept_out_of_previous_and_journaled() {
    let b = boot("watch-corrupt-current");
    let older_good = profile_doc_ceiling("qwen", 512);
    b.seed("qwen.previous.json", &older_good);
    b.seed("qwen.json", "{ truncated json");
    let (runner, _probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|_r, _c| exited(0));

    b.run(&runner, &gate);

    assert_eq!(
        b.read("qwen.previous.json"),
        older_good,
        "the previous reference already on disk survives untouched"
    );
    assert!(
        b.events().iter().any(|e| matches!(e,
            Event::Degraded { reason }
                if reason.contains("qwen.json") && reason.contains("drift"))),
        "the unpromotable document must be named in the journal: {:?}",
        b.events()
    );
}

/// Spec §5's bound: retention keeps the latest N transients per model, and a
/// file this daemon deleted is a fact about the evidence trail — journaled,
/// never quiet housekeeping.
#[test]
fn a_dropped_transient_is_journaled_by_name() {
    let b = boot("watch-transient-bound");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024));
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    // Fill the bound with older confirm documents from earlier boots.
    let old = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    for i in 0..MAX_TRANSIENTS {
        let name = format!("qwen.transient-0000000{i}.json");
        b.seed(&name, &profile_doc_ceiling("qwen", 100 + i as u32));
        set_mtime(&b.profiles.join(&name), old);
    }
    let (runner, _probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|reference, current| {
        if reference.ends_with(".previous.json") && !is_transient(current) {
            exited(1)
        } else {
            exited(0)
        }
    });

    b.run(&runner, &gate);

    assert_eq!(
        b.transients().len(),
        MAX_TRANSIENTS,
        "the bound holds after the confirm run files its document"
    );
    assert!(
        b.events().iter().any(|e| matches!(e,
            Event::Degraded { reason }
                if reason.contains("qwen.transient-00000000.json"))),
        "the dropped document must be named: {:?}",
        b.events()
    );
}

// ---------------------------------------------------------------------------
// Verdict-gated-admission, end to end
// (`docs/superpowers/specs/2026-08-18-verdict-gated-admission-design.md` §7,
// "the single most important test").
//
// Every test above proves the drift watch measures correctly by feeding it
// documents and reading `ModelDrift` back. Neither of these two feeds
// `set_drift` directly, and neither hand-builds a `DriftStatus`: both drive
// `watch_model -> set_drift -> admission_block` through the real boot path
// (`Boot::run` -> `run_post_with_gate`), the way `main.rs` actually calls it,
// and then go one step further than every test above by asking the pager for
// an admission decision — `create_agent` — against what that boot produced.
// ---------------------------------------------------------------------------

/// THE fleet guard, run for real. assay upgrades move every model's
/// instrument identity at once (spec §3: "never a pass, never a fail"), and
/// slice 1 §8's committed mixed-version fixtures are the real bytes that
/// meet a daemon on the first boot after one: `fixtures/profile-v4-qwen3-8b.json`
/// (the pre-upgrade schema, instrument `"0.5.0/v4"`) seeded as both this
/// model's blessed baseline and last boot's document, against
/// `fixtures/profile-v8-qwen3-8b.json` (instrument `"0.9.0/v8"`) as this
/// boot's measurement — the same V4/V8 pair
/// `a_changed_instrument_is_named_before_the_diff_is_ever_spawned` above pins
/// at the gate level, driven here through the full orchestration instead.
/// Registered under the fixtures' own model name (`boot_for`, not `boot`):
/// `PostRunner::probe` refuses a document whose `model.name` does not match
/// the model it was asked to probe, so relabelling these bytes as `"qwen"`
/// would never reach the watch at all.
///
/// The diff gate is scripted to answer exit 1 — drift — if it is EVER
/// spawned, so a precheck that got bypassed would not read as a quiet
/// no-op: it would read as the fleet blocked, which is the failure this test
/// exists to catch.
#[test]
fn an_instrument_upgrade_never_blocks_the_fleet_end_to_end() {
    let b = boot_for("watch-fleet-guard-e2e", "qwen3:8b");
    b.seed("qwen3:8b.json", V4_QWEN3_8B); // last boot's -> becomes the step reference
    b.seed("qwen3:8b.baseline.json", V4_QWEN3_8B); // the blessed cumulative reference
    let (runner, probes) = scripted_probes(vec![Ok(V8_QWEN3_8B.to_string())]);
    let (gate, calls) = gate_deciding(|_reference, _current| exited(1));

    b.run(&runner, &gate);

    assert!(
        calls.borrow().is_empty(),
        "an instrument change must be named before the diff is ever spawned, on BOTH \
         comparisons, got {:?}",
        calls.borrow()
    );
    assert_eq!(
        probes.borrow().len(),
        1,
        "InstrumentChanged settles on the first reading; there is nothing to confirm"
    );
    let expected = DriftStatus::InstrumentChanged {
        reference: "0.5.0/v4".to_string(),
        current: "0.9.0/v8".to_string(),
    };
    assert_eq!(
        b.drift(),
        Some(ModelDrift {
            step: expected.clone(),
            cumulative: expected,
        }),
        "both comparisons read the instrument change independently"
    );

    let mut p = b.pager.lock().unwrap();
    assert!(
        p.admission_block_for("qwen3:8b").is_none(),
        "an instrument change must never derive a block"
    );
    p.create_agent("qwen3:8b", 50, None, 10_000)
        .expect("an assay upgrade must never take a model out of admission");
}

/// The block, run for real: a same-instrument pair where the CUMULATIVE
/// comparison drifts and the confirm reproduces it — spec §4's
/// confirm-then-alarm settling on `Confirmed`, and verdict-gated-admission
/// design §2's derivation from it, both through the real boot path this time
/// rather than a `set_drift` call built by hand.
#[test]
fn a_confirmed_cumulative_regression_blocks_admission_end_to_end() {
    let b = boot("watch-cumulative-blocks-e2e");
    let last_boot = profile_doc_ceiling("qwen", 1024);
    let baseline = profile_doc_ceiling("qwen", 900);
    b.seed("qwen.json", &last_boot);
    b.seed("qwen.baseline.json", &baseline);
    let (runner, probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    // The cumulative reference (baseline) drifts every time it is asked; the
    // step reference (previous) does not — so only the cumulative comparison
    // has a hypothesis to confirm.
    let (gate, _calls) = gate_deciding(|reference, _current| {
        if reference.ends_with(".baseline.json") {
            exited(1)
        } else {
            exited(0)
        }
    });

    b.run(&runner, &gate);

    assert_eq!(
        probes.borrow().len(),
        2,
        "the boot probe plus exactly one confirm — never zero, never a retry loop"
    );
    let block_reference = sha8(&baseline);
    assert_eq!(
        b.drift(),
        Some(ModelDrift {
            step: DriftStatus::WithinNoise,
            cumulative: DriftStatus::Confirmed {
                reference: block_reference.clone(),
            },
        })
    );

    let admission_rows: Vec<(String, String, String, String)> = b
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Admission {
                model,
                action,
                reference,
                provenance,
            } => Some((model, action, reference, provenance)),
            _ => None,
        })
        .collect();
    assert_eq!(
        admission_rows,
        vec![(
            "qwen".to_string(),
            "blocked".to_string(),
            block_reference.clone(),
            "drift-watch".to_string(),
        )],
        "exactly one blocked row, with the watch's own provenance: {admission_rows:?}"
    );

    let mut p = b.pager.lock().unwrap();
    let block = p
        .admission_block_for("qwen")
        .expect("the confirmed cumulative regression must stand as a block")
        .clone();
    assert_eq!(block.reference, block_reference);

    match p.create_agent("qwen", 50, None, 10_000).unwrap_err() {
        PagerError::DriftBlocked { model, reference } => {
            assert_eq!(model, "qwen");
            assert_eq!(reference, block_reference);
        }
        other => panic!("expected DriftBlocked, got {other:?}"),
    }

    p.clear_admission_block("qwen").unwrap();
    assert!(p.admission_block_for("qwen").is_none());
    p.create_agent("qwen", 50, None, 10_000)
        .expect("clearing the block re-admits");
}
