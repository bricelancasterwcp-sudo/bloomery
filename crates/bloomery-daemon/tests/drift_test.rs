//! `ProfileStore` mechanics: naming, rotation, blessing, transient retention.
//!
//! The store's on-disk contract -- which path POST actually writes, what
//! `rotate` moves, what `bless` copies, and how the four transient slots age
//! out.
//!
//! **Split 2026-09-01** (carried-debt slice D): this file was 1983 lines, the
//! second-worst offender against the 800-line ceiling. The drift GATE is in
//! `drift_gate_test.rs`, its boot wiring in `drift_boot_test.rs`, and the
//! verdict-gated-admission arc in `drift_admission_test.rs`. Fixtures reached
//! by more than one of them are in `tests/common/drift.rs`.

mod common;

use bloomery_core::journal::{replay, sha256_hex, sha256_hex_bytes, Event, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::drift::{profile_file_name, DriftError, Rotation, MAX_TRANSIENTS};
use bloomery_daemon::pager::Pager;
use bloomery_daemon::post::PostRunner;
use bloomery_substrate::fake::FakeSubstrate;
use std::path::Path;
use std::time::{Duration, SystemTime};

use common::drift::{profile_doc, qwen_like_meta, set_mtime, store_in, tier};

fn mtime(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .expect("metadata")
        .modified()
        .expect("mtime")
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
