//! POST (power-on self test) tests: the `assay` subprocess runner and the
//! profile-gated admission it feeds.
//!
//! Every test here is GPU-free and python-free — [`PostRunner::with_runner`]
//! injects a fake command runner, so the *invocation* and the *failure
//! classification* are pinned without ever spawning assay. The first three
//! tests are the Task 16 brief's own, verbatim.
//!
//! The admission half drives a real `Pager<FakeSubstrate>`: law 5 says no
//! model gets work without a measured profile, and the three ways a model
//! can still be admitted (a real profile, the boot-time `posting` window,
//! or the operator's `allow_unprofiled` override) each leave a different,
//! checkable record.

use bloomery_core::journal::{replay, Event, Journal};
use bloomery_core::profile::Profile;
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::Tier;
use bloomery_daemon::pager::{Pager, PagerError};
use bloomery_daemon::post::{PostError, PostRunner};
use bloomery_substrate::fake::FakeSubstrate;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// The Task 16 brief's three tests, verbatim.
// ---------------------------------------------------------------------------

fn fake_output(code: i32, stdout: &str, stderr: &str) -> std::process::Output {
    use std::os::unix::process::ExitStatusExt;
    std::process::Output {
        status: std::process::ExitStatus::from_raw(code << 8),
        stdout: stdout.into(),
        stderr: stderr.into(),
    }
}

#[test]
fn probe_success_parses_written_profile() {
    let dir = std::env::temp_dir().join("bloomery-post-test");
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("qwen.json");
    let profile_json = r#"{"assay_profile_version":3,"probe_version":"0.4.1","model":{"name":"qwen"},"verdicts":{}}"#;
    let out_clone = out.clone();
    let runner = PostRunner::with_runner(Box::new(move |_py, _args| {
        std::fs::write(&out_clone, profile_json).unwrap(); // assay writes --json path
        Ok(fake_output(0, "", ""))
    }));
    let tier = Tier {
        name: "enthusiast-16gb".into(),
        emulated: false,
    };
    let p = runner.probe(8181, "qwen", &tier, &out).unwrap();
    assert_eq!(p.model_name(), "qwen");
}

#[test]
fn nonzero_exit_is_named_infrastructure_failure() {
    let runner = PostRunner::with_runner(Box::new(|_, _| Ok(fake_output(4, "", "no daemon"))));
    let tier = Tier {
        name: "t".into(),
        emulated: true,
    };
    let out = std::env::temp_dir()
        .join("bloomery-post-test")
        .join("x.json");
    match runner.probe(8181, "m", &tier, &out) {
        Err(PostError::NonZeroExit { code: 4, stderr }) => assert!(stderr.contains("no daemon")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn command_line_is_exactly_the_documented_invocation() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen2 = seen.clone();
    let runner = PostRunner::with_runner(Box::new(move |py, args| {
        let mut v = vec![py.to_string()];
        v.extend(args.iter().cloned());
        *seen2.lock().unwrap() = v;
        Err(std::io::Error::from(std::io::ErrorKind::NotFound)) // stop after capture
    }));
    let tier = Tier {
        name: "enthusiast-16gb".into(),
        emulated: false,
    };
    let out = std::path::PathBuf::from("/tmp/p.json");
    let _ = runner.probe(9999, "qwen", &tier, &out);
    let cmd = seen.lock().unwrap().join(" ");
    assert!(cmd.contains("-m assay probe http://127.0.0.1:9999"));
    assert!(cmd.contains("--model qwen"));
    assert!(cmd.contains("--backend openai"));
    assert!(cmd.contains("--quick"));
    assert!(cmd.contains("--tier enthusiast-16gb"));
    assert!(cmd.contains("--real-hardware"));
    assert!(!cmd.contains("--emulated"));
}

// ---------------------------------------------------------------------------
// The rest of the runner's surface: the emulated marking, and the two
// failure classes the brief's tests don't reach.
// ---------------------------------------------------------------------------

/// The tier marking is not decoration: assay refuses `--tier` without one of
/// the two marks precisely so an emulated number can never masquerade as
/// real hardware. The `false` case is pinned above; this is its mirror.
#[test]
fn an_emulated_tier_is_marked_emulated_not_real_hardware() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let seen2 = seen.clone();
    let runner = PostRunner::with_runner(Box::new(move |py, args| {
        *seen2.lock().unwrap() = format!("{py} {}", args.join(" "));
        Err(std::io::Error::from(std::io::ErrorKind::NotFound))
    }));
    let tier = Tier {
        name: "average-gamer-8gb".into(),
        emulated: true,
    };
    let _ = runner.probe(8181, "qwen", &tier, &PathBuf::from("/tmp/p.json"));
    let cmd = seen.lock().unwrap().clone();
    assert!(cmd.contains("--tier average-gamer-8gb"));
    assert!(cmd.contains("--emulated"));
    assert!(!cmd.contains("--real-hardware"));
}

/// A runner that cannot even start the process is `Spawn`, not a silent
/// "no profile": the operator needs to know assay is missing, not guess.
#[test]
fn a_failed_spawn_is_named_spawn_with_the_os_reason() {
    let runner = PostRunner::with_runner(Box::new(|_, _| {
        Err(std::io::Error::from(std::io::ErrorKind::NotFound))
    }));
    let tier = Tier {
        name: "t".into(),
        emulated: true,
    };
    match runner.probe(8181, "m", &tier, &PathBuf::from("/tmp/never-written.json")) {
        Err(PostError::Spawn(msg)) => assert!(
            msg.contains("not found") || msg.contains("NotFound"),
            "spawn error should carry the OS reason, got {msg:?}"
        ),
        other => panic!("{other:?}"),
    }
}

/// assay exiting 0 without writing its `--json` document is a broken
/// instrument, not a profile-less success — the profile is read back from
/// the file, never from stdout, so there is nothing to fall back to.
#[test]
fn exit_zero_without_the_json_document_is_a_bad_profile() {
    let out = std::env::temp_dir().join(format!(
        "bloomery-post-missing-{}-{}.json",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_file(&out);
    let runner = PostRunner::with_runner(Box::new(|_, _| Ok(fake_output(0, "profile!", ""))));
    let tier = Tier {
        name: "t".into(),
        emulated: true,
    };
    match runner.probe(8181, "m", &tier, &out) {
        Err(PostError::BadProfile(msg)) => assert!(
            msg.contains(&out.display().to_string()),
            "BadProfile should name the path it could not read, got {msg:?}"
        ),
        other => panic!("{other:?}"),
    }
}

/// A written-but-unusable document (here: schema v1, which `Profile` refuses)
/// is `BadProfile` too — an unreadable profile must never be treated as an
/// absent one and silently downgraded to "unprofiled but fine".
#[test]
fn an_unsupported_schema_is_a_bad_profile() {
    let dir = std::env::temp_dir().join("bloomery-post-test");
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join(format!("v1-{}.json", std::process::id()));
    let out_clone = out.clone();
    let runner = PostRunner::with_runner(Box::new(move |_, _| {
        std::fs::write(
            &out_clone,
            r#"{"assay_profile_version":1,"probe_version":"0.1.0","model":{"name":"m"}}"#,
        )
        .unwrap();
        Ok(fake_output(0, "", ""))
    }));
    let tier = Tier {
        name: "t".into(),
        emulated: true,
    };
    match runner.probe(8181, "m", &tier, &out) {
        Err(PostError::BadProfile(msg)) => assert!(msg.contains("schema")),
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Profile-gated admission (law 5) — the pager half.
// ---------------------------------------------------------------------------

fn meta() -> bloomery_core::gguf::GgufMeta {
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

fn profile_with_ceiling(name: &str, max_verified: u32) -> Profile {
    Profile::from_json(&format!(
        r#"{{"assay_profile_version":3,"probe_version":"0.4.1","model":{{"name":"{name}"}},
            "ceiling":{{"max_verified":{max_verified}}},"verdicts":{{}}}}"#
    ))
    .expect("fixture profile parses")
}

/// A pager with one registered, **unprofiled** `qwen`, plus the path to its
/// journal so a test can replay what the admission decision recorded, and
/// its scratch directory (where POST writes profile documents).
fn unprofiled_pager(tag: &str) -> (Pager<FakeSubstrate>, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("bloomery-post-adm-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let jpath = dir.join("j.jsonl");
    let journal = Journal::open(&jpath).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut p = Pager::new(
        FakeSubstrate::new(),
        journal,
        images,
        Box::new(|| Some(10u64.pow(9))),
    );
    let gguf = dir.join("qwen.gguf");
    std::fs::write(&gguf, b"weights").unwrap();
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    (p, jpath, dir)
}

/// Registers a second, unprofiled model so a test can watch POST handle
/// several of them.
fn register_extra(p: &mut Pager<FakeSubstrate>, dir: &Path, name: &str) {
    let gguf = dir.join(format!("{name}.gguf"));
    std::fs::write(&gguf, name.as_bytes()).unwrap();
    p.register_model(name, &gguf, meta(), None).unwrap();
}

#[test]
fn unprofiled_model_is_refused_when_allow_unprofiled_is_false() {
    let (mut p, _j, _dir) = unprofiled_pager("refused");
    p.set_allow_unprofiled(false);
    match p.create_agent("qwen", 100, None, 1000) {
        Err(PagerError::Unprofiled(model)) => assert_eq!(model, "qwen"),
        other => panic!("expected Unprofiled, got {other:?}"),
    }
}

#[test]
fn allow_unprofiled_admits_and_journals_a_degraded_naming_the_model() {
    let (mut p, jpath, _dir) = unprofiled_pager("allowed");
    p.set_allow_unprofiled(true);
    p.create_agent("qwen", 100, None, 1000)
        .expect("allow_unprofiled admits");
    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Degraded { reason } if reason.contains("qwen") && reason.contains("profile"))),
        "expected a Degraded naming qwen, got {events:?}"
    );
}

/// The chicken-and-egg resolution: assay must be able to drive `/v1` before
/// any profile exists, so `posting` admits unprofiled models — and the
/// window closes when POST finishes, restoring law 5.
#[test]
fn posting_admits_unprofiled_and_normal_admission_returns_when_it_clears() {
    let (mut p, _j, _dir) = unprofiled_pager("posting");
    p.set_allow_unprofiled(false);
    p.set_posting(true);
    p.create_agent("qwen", 100, None, 1000)
        .expect("provisional admission during POST");
    p.set_posting(false);
    match p.create_agent("qwen", 100, None, 1000) {
        Err(PagerError::Unprofiled(model)) => assert_eq!(model, "qwen"),
        other => panic!("expected Unprofiled once POST cleared, got {other:?}"),
    }
}

/// The provisional window is a *stated* suspension of law 5, not a silent
/// one: entering it leaves a record naming what was suspended.
#[test]
fn entering_the_posting_window_journals_the_provisional_admission() {
    let (mut p, jpath, _dir) = unprofiled_pager("posting-journal");
    p.set_posting(true);
    p.create_agent("qwen", 100, None, 1000).unwrap();
    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Degraded { reason } if reason.contains("qwen") && reason.contains("POST"))),
        "expected a Degraded naming the provisional admission, got {events:?}"
    );
}

#[test]
fn attaching_a_profile_lifts_the_gate_without_allow_unprofiled() {
    let (mut p, _j, _dir) = unprofiled_pager("attach");
    p.set_allow_unprofiled(false);
    assert!(p.create_agent("qwen", 100, None, 1000).is_err());
    p.attach_profile("qwen", profile_with_ceiling("qwen", 512), true)
        .expect("attach to a registered model");
    p.create_agent("qwen", 100, None, 1000)
        .expect("a profiled model is admitted");
}

/// An **externally** measured ceiling is not cosmetic: it has to reach the
/// window law, or a "profiled" model would be admitted on geometry nobody
/// measured. (The self-measured case is the opposite — see below.)
#[test]
fn an_external_ceiling_binds_the_window() {
    let (mut p, _j, _dir) = unprofiled_pager("ceiling");
    p.attach_profile("qwen", profile_with_ceiling("qwen", 512), false)
        .unwrap();
    let info = p.create_agent("qwen", 100, None, 1000).unwrap();
    assert_eq!(info.window_tokens, 512);
    assert_eq!(info.bound_by, "measured_ceiling");
}

/// The anti-ratchet rule (controller ruling, Task 16). A profile this
/// daemon produced by probing *itself* measured its own refusal gate, not
/// the model: feeding that ceiling back into the window law double-applies
/// the same conservatism, and each re-probe would measure a lower ceiling
/// than the last. So a self-measured ceiling is ignored by the geometry —
/// while everything else in the profile (verdicts, and the fact that the
/// model counts as profiled at all) is kept.
#[test]
fn a_self_measured_ceiling_does_not_clamp_the_window() {
    let (mut p, _j, _dir) = unprofiled_pager("self-ceiling");
    p.set_allow_unprofiled(false);
    p.attach_profile("qwen", profile_with_ceiling("qwen", 512), true)
        .unwrap();
    let info = p.create_agent("qwen", 100, None, 1000).unwrap();
    assert_eq!(
        info.bound_by, "training_ctx",
        "a self-probe's ceiling must not bind the window law"
    );
    assert_eq!(info.window_tokens, 4096);
}

/// A profile supplied at registration comes from outside this daemon (an
/// operator, a previous run's externally-validated document), so it keeps
/// the clamping behavior — the `register_model` half of the same rule.
#[test]
fn a_profile_supplied_at_registration_is_external_and_binds() {
    let (mut p, _j, dir) = unprofiled_pager("register-external");
    let gguf = dir.join("granite.gguf");
    std::fs::write(&gguf, b"weights").unwrap();
    p.register_model(
        "granite",
        &gguf,
        meta(),
        Some(profile_with_ceiling("granite", 512)),
    )
    .unwrap();
    let info = p.create_agent("granite", 100, None, 1000).unwrap();
    assert_eq!(info.bound_by, "measured_ceiling");
    assert_eq!(info.window_tokens, 512);
}

#[test]
fn attaching_a_profile_to_an_unregistered_model_is_named() {
    let (mut p, _j, _dir) = unprofiled_pager("attach-unknown");
    match p.attach_profile("nope", profile_with_ceiling("nope", 512), false) {
        Err(PagerError::UnknownModel(m)) => assert_eq!(m, "nope"),
        other => panic!("expected UnknownModel, got {other:?}"),
    }
}

/// Status is what an operator reads to answer "why is this daemon refusing
/// everything" — so the tier it was told about, whether POST is still
/// running, and which models actually carry a profile all have to be in it.
#[test]
fn status_reports_the_tier_the_posting_flag_and_which_models_are_profiled() {
    let (mut p, _j, _dir) = unprofiled_pager("status");
    assert!(
        p.status().tier.is_none(),
        "an unwired tier is None, never an invented name"
    );
    p.set_tier("enthusiast-16gb", false);
    p.set_posting(true);

    let s = p.status();
    let tier = s.tier.expect("tier was wired");
    assert_eq!(tier.name, "enthusiast-16gb");
    assert!(!tier.emulated);
    assert!(s.posting);
    assert!(!s.models[0].profiled);

    p.attach_profile("qwen", profile_with_ceiling("qwen", 512), true)
        .unwrap();
    p.set_posting(false);
    let s = p.status();
    assert!(!s.posting);
    assert!(s.models[0].profiled);
}

// ---------------------------------------------------------------------------
// The boot sequence itself: `run_post` against a real `Pager<FakeSubstrate>`
// and a fake assay. No python, no GPU, no socket — the sequence is the same
// one `main.rs` runs on a real boot.
// ---------------------------------------------------------------------------

/// A fake assay: writes a real profile document to whatever `--json` path it
/// was handed (exactly as assay does) for every model in `succeed`, and
/// exits 4 with a reason for any other.
fn scripted_assay(succeed: &[&str]) -> PostRunner {
    let succeed: Vec<String> = succeed.iter().map(|s| (*s).to_string()).collect();
    PostRunner::with_runner(Box::new(move |_py, args| {
        let value_of = |flag: &str| -> String {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_default()
        };
        let model = value_of("--model");
        if !succeed.contains(&model) {
            return Ok(fake_output(4, "", &format!("cannot reach model {model}")));
        }
        std::fs::write(
            value_of("--json"),
            format!(
                r#"{{"assay_profile_version":3,"probe_version":"0.4.1","model":{{"name":"{model}"}},
                     "ceiling":{{"max_verified":2048}},"verdicts":{{}}}}"#
            ),
        )
        .unwrap();
        Ok(fake_output(0, "", ""))
    }))
}

fn tier() -> Tier {
    Tier {
        name: "enthusiast-16gb".into(),
        emulated: false,
    }
}

#[test]
fn post_attaches_each_profile_journals_ok_and_closes_the_window() {
    let (mut pager, jpath, dir) = unprofiled_pager("run-ok");
    pager.set_allow_unprofiled(false);
    pager.set_posting(true);
    let pager = std::sync::Mutex::new(pager);

    bloomery_daemon::post::run_post(
        &pager,
        &scripted_assay(&["qwen"]),
        &["qwen".to_string()],
        8181,
        &tier(),
        &dir,
    )
    .expect("POST records its result");

    let mut p = pager.lock().unwrap();
    let status = p.status();
    assert!(!status.posting, "the provisional window closes with POST");
    assert!(status.models[0].profiled);
    // Law 5 is back in force *and* satisfied: admission now succeeds on the
    // profile, not on the suspension. The window is NOT clamped by the
    // ceiling POST just measured (2048, below this fixture's 4096 training
    // context): that ceiling is this daemon measuring its own refusal gate,
    // and clamping by it would ratchet the window down on every re-probe.
    let info = p.create_agent("qwen", 100, None, 1000).unwrap();
    assert_eq!(info.bound_by, "training_ctx");
    assert_eq!(info.window_tokens, 4096);

    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Post { model, outcome, profile_path: Some(path) }
                if model == "qwen" && outcome == "ok" && path.ends_with("qwen.json"))),
        "expected Post{{ok}} naming the document it attached, got {events:?}"
    );
}

#[test]
fn a_failed_probe_leaves_the_model_unprofiled_and_says_so_twice() {
    let (mut pager, jpath, dir) = unprofiled_pager("run-fail");
    pager.set_allow_unprofiled(false);
    pager.set_posting(true);
    let pager = std::sync::Mutex::new(pager);

    bloomery_daemon::post::run_post(
        &pager,
        &scripted_assay(&[]), // every probe fails
        &["qwen".to_string()],
        8181,
        &tier(),
        &dir,
    )
    .expect("a failed probe is still recorded successfully");

    let mut p = pager.lock().unwrap();
    assert!(
        !p.status().posting,
        "the window closes even when POST fails"
    );
    assert!(!p.status().models[0].profiled);
    // Degraded boot, stated: the model is refused rather than served on
    // capabilities nobody measured.
    match p.create_agent("qwen", 100, None, 1000) {
        Err(PagerError::Unprofiled(m)) => assert_eq!(m, "qwen"),
        other => panic!("expected Unprofiled after a failed POST, got {other:?}"),
    }

    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Post { model, outcome, profile_path: None }
                if model == "qwen" && outcome.starts_with("failed:")
                    && outcome.contains("cannot reach model"))),
        "expected Post{{failed}} carrying assay's own stderr, got {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Degraded { reason } if reason.contains("POST failed for qwen"))),
        "expected a Degraded for the failed model, got {events:?}"
    );
}

/// A two-model daemon where assay can only profile one boots degraded for
/// that one and fully admitted for the other — more useful, and more honest,
/// than an all-or-nothing boot.
#[test]
fn one_models_failure_does_not_stop_the_others() {
    let (mut pager, jpath, dir) = unprofiled_pager("run-mixed");
    register_extra(&mut pager, &dir, "granite");
    pager.set_allow_unprofiled(false);
    pager.set_posting(true);
    let pager = std::sync::Mutex::new(pager);

    bloomery_daemon::post::run_post(
        &pager,
        &scripted_assay(&["qwen"]),
        &["granite".to_string(), "qwen".to_string()],
        8181,
        &tier(),
        &dir,
    )
    .unwrap();

    let p = pager.lock().unwrap();
    let models = p.status().models;
    let granite = models.iter().find(|m| m.name == "granite").unwrap();
    let qwen = models.iter().find(|m| m.name == "qwen").unwrap();
    assert!(!granite.profiled, "the failing model stays unprofiled");
    assert!(qwen.profiled, "the model probed after it is still profiled");

    let events = replay(&jpath).unwrap();
    let outcomes: Vec<(&str, &str)> = events
        .iter()
        .filter_map(|e| match e {
            Event::Post { model, outcome, .. } => Some((model.as_str(), outcome.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(outcomes.len(), 2, "both models are recorded: {outcomes:?}");
    assert!(outcomes.iter().any(|(m, o)| *m == "qwen" && *o == "ok"));
    assert!(outcomes
        .iter()
        .any(|(m, o)| *m == "granite" && o.starts_with("failed:")));
}

/// The brief's verbatim invocation test asserts the command *contains*
/// `http://127.0.0.1:9999`, which a `/v1`-suffixed URL satisfies. That
/// suffix is load-bearing and was measured, not guessed: assay's
/// OpenAI-compatible backend appends `/chat/completions` to whatever base
/// URL it is given, and the first live boot smoke journaled a POST failure
/// reading `HTTP 404 from http://127.0.0.1:8401/chat/completions` without
/// it. Pinned separately so nobody "simplifies" it back out.
#[test]
fn the_probe_url_points_at_the_v1_surface_not_the_root() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let seen2 = seen.clone();
    let runner = PostRunner::with_runner(Box::new(move |_py, args| {
        *seen2.lock().unwrap() = args.join(" ");
        Err(std::io::Error::from(std::io::ErrorKind::NotFound))
    }));
    let _ = runner.probe(9999, "qwen", &tier(), &PathBuf::from("/tmp/p.json"));
    let cmd = seen.lock().unwrap().clone();
    assert!(
        cmd.contains("probe http://127.0.0.1:9999/v1 "),
        "assay must be pointed at /v1, got: {cmd}"
    );
}

// ---------------------------------------------------------------------------
// Review round: subprocess bounding, stale-document guard, dedup pin.
// ---------------------------------------------------------------------------

/// A wedged assay must not hold the provisional-admission window open
/// forever — the exact failure `post.rs`'s own docs say must never happen.
/// This drives the **real** spawn layer (the injectable runner replaces the
/// subprocess entirely, so it cannot exercise this), with a short timeout
/// standing in for the shipped 600 s cap, and proves both halves: the
/// expiry is named, and the child is actually killed rather than abandoned.
#[test]
fn a_wedged_probe_is_killed_and_named_a_timeout() {
    let marker = std::env::temp_dir().join(format!(
        "bloomery-post-timeout-{}-{}.marker",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_file(&marker);
    let script = format!("sleep 1; echo x > {}", marker.display());
    let started = std::time::Instant::now();

    let err = bloomery_daemon::post::run_bounded_for_test(
        "/bin/sh",
        &["-c".to_string(), script],
        std::time::Duration::from_millis(300),
    )
    .expect_err("a child that outlives the cap must not return Ok");

    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    assert!(
        err.to_string().contains("timed out"),
        "the expiry must be named: {err}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "the cap must bound the wait, not the child"
    );

    // The child would write the marker at t+1s. It never gets there,
    // because it was killed — not merely stopped being waited on.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    assert!(
        !marker.exists(),
        "the timed-out child kept running after the cap expired"
    );
}

/// A leftover document from a previous boot must never be mistaken for what
/// assay just wrote. Without removing it first, an assay that exits 0
/// having written nothing (a crash after its own success path, a `--json`
/// path it could not write) would silently attach yesterday's measurements
/// as today's.
#[test]
fn a_stale_json_document_is_not_attached_as_a_fresh_profile() {
    let out = std::env::temp_dir().join(format!(
        "bloomery-post-stale-{}-{}.json",
        std::process::id(),
        line!()
    ));
    std::fs::write(
        &out,
        r#"{"assay_profile_version":3,"probe_version":"0.4.1","model":{"name":"qwen"},
            "ceiling":{"max_verified":99999},"verdicts":{}}"#,
    )
    .unwrap();

    let runner = PostRunner::with_runner(Box::new(|_, _| Ok(fake_output(0, "", ""))));
    match runner.probe(8181, "qwen", &tier(), &out) {
        Err(PostError::BadProfile(msg)) => assert!(msg.contains(&out.display().to_string())),
        other => panic!("a stale document must not become a profile, got {other:?}"),
    }
    assert!(
        !out.exists(),
        "the stale document must be gone, not left to be re-read next boot"
    );
}

/// The constructor plumbing is the load-bearing line: `PostRunner::new`'s
/// `probe_timeout` argument must actually reach the spawn layer, not just
/// exist. Drives a real slow child (a shell script that ignores every
/// argument assay's invocation would pass it) through the *public* `new` +
/// `probe` surface — the same real-subprocess seam as
/// `a_wedged_probe_is_killed_and_named_a_timeout` above, but proving the
/// argument actually reaches `run_bounded` rather than being ignored in
/// favor of the old hardcoded 600s default. If `new` dropped the argument,
/// the 1s child would run to completion (well inside this test's own
/// patience), the process would exit 0, and the probe would fail on a
/// missing `--json` document instead — a different error, not this one —
/// which is exactly the failure mode this test exists to catch.
#[test]
fn post_runner_new_honors_its_configured_probe_timeout() {
    let dir = std::env::temp_dir().join("bloomery-post-ctor-timeout-test");
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join(format!("marker-{}-{}", std::process::id(), line!()));
    let script = dir.join(format!("slow-python-{}-{}.sh", std::process::id(), line!()));
    let _ = std::fs::remove_file(&marker);
    std::fs::write(
        &script,
        format!("#!/bin/sh\nsleep 1\necho x > {}\n", marker.display()),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&script).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&script, perm).unwrap();
    }

    let runner = PostRunner::new(
        script.display().to_string(),
        std::time::Duration::from_millis(300),
    );
    let tier = Tier {
        name: "t".into(),
        emulated: true,
    };
    let started = std::time::Instant::now();
    let out = dir.join(format!("never-written-{}.json", line!()));
    match runner.probe(8181, "m", &tier, &out) {
        Err(PostError::Spawn(msg)) => assert!(
            msg.contains("timed out"),
            "expected the configured 300ms timeout to fire, got {msg:?}"
        ),
        other => panic!(
            "expected a timeout-driven Spawn error from the configured 300ms cap, got {other:?}"
        ),
    }
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "the configured 300ms cap must bound the wait, not the 1s child"
    );
    // The child would write the marker at t+1s. A runner that ignored the
    // configured timeout would let it run to completion and write it.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    assert!(
        !marker.exists(),
        "a runner that honors its configured timeout must kill the child \
         before it writes the marker"
    );
}

/// The degradation is said once per model, not once per agent: a busy
/// `allow_unprofiled` daemon (or POST's ~75 calls) would otherwise bury the
/// journal in one repeated sentence. Kills the delete-the-dedup mutant.
#[test]
fn the_unprofiled_degradation_is_said_once_per_model_not_once_per_agent() {
    let (mut p, jpath, _dir) = unprofiled_pager("dedup");
    p.set_allow_unprofiled(true);
    p.create_agent("qwen", 100, None, 1000).unwrap();
    p.create_agent("qwen", 100, None, 1000).unwrap();
    p.create_agent("qwen", 100, None, 1000).unwrap();

    let said = replay(&jpath)
        .unwrap()
        .iter()
        .filter(|e| matches!(e, Event::Degraded { reason } if reason.contains("qwen")))
        .count();
    assert_eq!(said, 1, "three agents, one sentence");
}
