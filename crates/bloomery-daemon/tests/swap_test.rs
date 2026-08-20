//! The swap-candidate seam: the cover gate (spec §3-§4) and the job that
//! drives it (§4's flow)
//! (`docs/superpowers/specs/2026-08-19-swap-candidate-seam-design.md`).
//!
//! No assay, no python, no GPU here: the gate's subprocess is injected, the
//! same seam `drift_test.rs` drives, so all four documented exit codes, the
//! undocumented-code path and the signal path are exercised with assay never
//! installed. The job half drives the real `run_candidate_probe` against a
//! real `Pager`, a real journal and a real profiles directory, with only the
//! two subprocesses (assay's probe, assay's cover) scripted.
//!
//! The process fixtures below (`Calls`, `exited`, `output`) are copied from
//! `drift_test.rs` rather than imported: each file under `tests/` is its own
//! crate, `tests/common/mod.rs` carries only the shared HTTP client, and the
//! crate's `test-support` module (`src/test_support.rs`) carries only the
//! ready-to-serve pager. Moving three-line wait-status helpers into either
//! would widen a shared surface for less than it costs to restate them.

use bloomery_core::gguf::parse_gguf_meta;
use bloomery_core::journal::{replay, sha256_hex_bytes, Event, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::Tier;
use bloomery_daemon::drift::ProfileStore;
use bloomery_daemon::pager::{Pager, PagerError};
use bloomery_daemon::post::PostRunner;
use bloomery_daemon::swap::{
    cover_argv, run_candidate_probe, scratch_identity, CoverGate, CoverOutcome, SwapSlot,
    SwapState, NOTE_HANDOVER, NOTE_TASK_GATES,
};
use bloomery_substrate::fake::FakeSubstrate;
use std::io::Write;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Every `(program, argv)` the gate spawned, in order. `Rc`/`RefCell` because
/// a [`bloomery_daemon::post::CommandRunner`] is deliberately not `Send` and
/// these tests are single-threaded.
type Calls = std::rc::Rc<std::cell::RefCell<Vec<(String, Vec<String>)>>>;

/// The signal `signalled` reports. Named rather than spelled `9` inline: the
/// number is a kernel constant, not a knob of this test.
const SIGKILL: i32 = 9;

/// A wait status carrying exit code `code` — the encoding `waitpid` returns.
fn exited(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code << 8)
}

/// A wait status for a child killed by `SIGKILL`: no exit code at all, which
/// is the case `exit_code: None` exists for. The same construction
/// `drift_test.rs::signalled` makes, with the signal fixed — this file has
/// exactly one signal case and nothing here varies by which signal it was.
fn signalled() -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(SIGKILL)
}

fn output(status: std::process::ExitStatus, stderr: &str) -> std::process::Output {
    std::process::Output {
        status,
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

/// A gate whose subprocess is scripted: it records every spawn and answers
/// with `status` and no stderr.
fn gate_answering(status: std::process::ExitStatus) -> (CoverGate, Calls) {
    gate_saying(status, "")
}

/// The same, with assay given words of its own — what the infrastructure
/// details carry through verbatim for the operator.
fn gate_saying(status: std::process::ExitStatus, stderr: &str) -> (CoverGate, Calls) {
    let calls: Calls = Calls::default();
    let sink = calls.clone();
    let stderr = stderr.to_string();
    let gate = CoverGate::with_runner(Box::new(move |program: &str, args: &[String]| {
        sink.borrow_mut().push((program.to_string(), args.to_vec()));
        Ok(output(status, &stderr))
    }));
    (gate, calls)
}

// ---------------------------------------------------------------------------
// The invocation
// ---------------------------------------------------------------------------

#[test]
fn cover_argv_is_the_documented_invocation() {
    let argv = cover_argv(Path::new("/d/floor.json"), Path::new("/d/cand.json"));
    assert_eq!(
        argv,
        vec!["-m", "assay", "cover", "/d/floor.json", "/d/cand.json"]
    );
}

#[test]
fn check_spawns_exactly_that_invocation_once() {
    let (gate, calls) = gate_answering(exited(0));
    gate.check(Path::new("/d/floor.json"), Path::new("/d/cand.json"));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(
        calls[0].1,
        cover_argv(Path::new("/d/floor.json"), Path::new("/d/cand.json"))
    );
}

// ---------------------------------------------------------------------------
// The four documented codes (spec §3)
// ---------------------------------------------------------------------------

#[test]
fn exit_zero_is_covered() {
    let (gate, _calls) = gate_answering(exited(0));
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(reading.outcome, CoverOutcome::Covered);
    assert_eq!(reading.exit_code, Some(0));
}

#[test]
fn exit_one_is_not_covered() {
    let (gate, _calls) = gate_answering(exited(1));
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(reading.outcome, CoverOutcome::NotCovered);
}

#[test]
fn exit_two_is_refused_and_never_a_pass() {
    let (gate, _calls) = gate_answering(exited(2));
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(
        reading.outcome,
        CoverOutcome::Refused {
            exit: 2,
            // A refusal that said nothing carries nothing: the empty string
            // is the expected shape, not a missing field.
            stderr: String::new(),
        }
    );
}

/// An assay too old to have the subcommand (anything < 0.13.0 under the
/// PYTHONPATH pin) makes `argparse` exit 2 with `invalid choice: 'cover'` —
/// byte-identical, by exit code alone, to a considered refusal about the
/// candidate. Carrying assay's sentence is the only thing that lets an
/// operator tell "your floor and candidate disagree on hardware class" from
/// "the tool you invoked has no cover command".
#[test]
fn a_missing_cover_subcommand_reads_refused_with_argparses_words() {
    let (gate, _calls) = gate_saying(exited(2), "invalid choice: 'cover'");
    match gate.check(Path::new("f"), Path::new("c")).outcome {
        CoverOutcome::Refused { exit, stderr } => {
            assert_eq!(exit, 2);
            assert!(stderr.contains("invalid choice"), "{stderr}");
        }
        other => panic!("expected Refused, got {other:?}"),
    }
}

#[test]
fn exit_three_is_incomplete_and_never_a_pass() {
    let (gate, _calls) = gate_answering(exited(3));
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(reading.outcome, CoverOutcome::Incomplete);
}

// ---------------------------------------------------------------------------
// Everything else is infrastructure, not a verdict
// ---------------------------------------------------------------------------

#[test]
fn an_undocumented_exit_is_infrastructure_not_a_verdict() {
    let (gate, _calls) = gate_answering(exited(7));
    match gate.check(Path::new("f"), Path::new("c")).outcome {
        CoverOutcome::Infra { detail } => {
            assert!(detail.contains("undocumented exit 7"), "{detail}");
        }
        other => panic!("expected Infra, got {other:?}"),
    }
}

/// Mirrors `drift_test.rs::a_spawn_failure_is_infra_naming_the_command` for
/// this gate: a runner that never reached a child at all must name what it
/// tried to run, and carry the OS's own words — nothing downstream can
/// reconstruct either from an outcome that only said "infrastructure".
#[test]
fn a_spawn_failure_is_infra_naming_the_command() {
    let gate = CoverGate::with_runner(Box::new(|_program, _args| {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file or directory",
        ))
    }));

    let reading = gate.check(Path::new("/d/floor.json"), Path::new("/d/cand.json"));

    match &reading.outcome {
        CoverOutcome::Infra { detail } => {
            assert!(
                detail.contains("python3"),
                "the interpreter must be named: {detail:?}"
            );
            assert!(
                detail.contains("assay") && detail.contains("cover"),
                "the failed command must be named: {detail:?}"
            );
            assert!(
                detail.contains("/d/floor.json") && detail.contains("/d/cand.json"),
                "the argv must be named: {detail:?}"
            );
            assert!(
                detail.contains("no such file"),
                "the OS's own words must survive: {detail:?}"
            );
        }
        other => panic!("expected Infra for a spawn failure, got {other:?}"),
    }
    assert_eq!(reading.exit_code, None);
}

#[test]
fn a_signal_killed_cover_has_no_exit_code_and_is_infrastructure() {
    let (gate, _calls) = gate_answering(signalled());
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(reading.exit_code, None);
    assert!(matches!(reading.outcome, CoverOutcome::Infra { .. }));
}

#[test]
fn assays_own_words_ride_along_with_an_infrastructure_detail() {
    let (gate, _calls) = gate_saying(exited(7), "cover: no such option");
    match gate.check(Path::new("f"), Path::new("c")).outcome {
        CoverOutcome::Infra { detail } => {
            assert!(detail.contains("cover: no such option"), "{detail}");
        }
        other => panic!("expected Infra, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The job — fixtures (design §4's flow)
//
// Every test below drives the real `run_candidate_probe` against a real
// `Pager`, a real journal and a real profiles directory. Only the two
// subprocesses are scripted: assay's probe (`PostRunner::with_runner`, the
// seam `drift_test.rs` drives) and assay's cover (`CoverGate::with_runner`,
// the seam above).
// ---------------------------------------------------------------------------

/// The configured model whose role the candidate would take.
const MODEL: &str = "qwen";

/// A scratch directory unique to this process and this call, so the
/// integration binary's concurrently-running tests never share one.
fn scratch(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("bloomery-swap-{}-{seq}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A minimal but real assay profile document — the same shape `drift_test.rs`
/// and `post_test.rs` feed their fake assay, so what parses there parses here.
fn profile_doc(model: &str) -> String {
    format!(
        r#"{{"assay_profile_version":3,"probe_version":"0.4.1","model":{{"name":"{model}"}},"ceiling":{{"max_verified":2048}},"verdicts":{{}}}}"#
    )
}

fn tier() -> Tier {
    Tier {
        name: "enthusiast-16gb".into(),
        emulated: false,
    }
}

fn kv_string(buf: &mut Vec<u8>, key: &str, val: &str) {
    buf.extend((key.len() as u64).to_le_bytes());
    buf.extend(key.as_bytes());
    buf.extend(8u32.to_le_bytes());
    buf.extend((val.len() as u64).to_le_bytes());
    buf.extend(val.as_bytes());
}

fn kv_u32(buf: &mut Vec<u8>, key: &str, val: u32) {
    buf.extend((key.len() as u64).to_le_bytes());
    buf.extend(key.as_bytes());
    buf.extend(4u32.to_le_bytes());
    buf.extend(val.to_le_bytes());
}

/// A **real, parseable** GGUF file (the header + KV shape
/// `bloomery_core::gguf::parse_gguf_meta` reads), copied from
/// `bloomery-core/tests/gguf_test.rs`.
///
/// Not the `b"weights"` placeholder the pager tests write: the job reuses
/// `main.rs`'s registration calls verbatim, GGUF metadata load included, so a
/// candidate that is not a GGUF never gets registered at all — which is its
/// own named failure, pinned below.
///
/// `name` goes into an otherwise-unread `general.name` key so two fixtures
/// differ in **bytes**, and therefore in digest, without differing in
/// geometry.
fn write_gguf(path: &Path, name: &str) {
    let mut kvs = Vec::new();
    kv_string(&mut kvs, "general.architecture", "qwen2");
    kv_string(&mut kvs, "general.name", name);
    kv_u32(&mut kvs, "qwen2.block_count", 28);
    kv_u32(&mut kvs, "qwen2.attention.head_count_kv", 4);
    kv_u32(&mut kvs, "qwen2.attention.key_length", 128);
    kv_u32(&mut kvs, "qwen2.context_length", 4096);
    let mut f = std::fs::File::create(path).expect("gguf fixture");
    f.write_all(b"GGUF").unwrap();
    f.write_all(&3u32.to_le_bytes()).unwrap();
    f.write_all(&0u64.to_le_bytes()).unwrap(); // tensor_count
    f.write_all(&6u64.to_le_bytes()).unwrap(); // kv_count
    f.write_all(&kvs).unwrap();
}

/// Where a fake assay was asked to write, once per probe, in order. The
/// *length* is load-bearing: one job probes the candidate exactly once.
type Probes = std::rc::Rc<std::cell::RefCell<Vec<PathBuf>>>;

fn value_of(args: &[String], flag: &str) -> String {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default()
}

/// A fake assay whose Nth probe follows `script[N]` (the last entry repeats):
/// `Ok(())` writes a real profile document **for whatever `--model` it was
/// handed** to the `--json` path and exits 0 — which is what makes the scratch
/// identity's document pass `PostRunner::probe`'s own model check — and
/// `Err(code)` exits `code` having written nothing, exactly as a failing probe
/// does. The same fixture as `drift_test.rs::scripted_probes`, with the
/// document derived rather than supplied.
fn scripted_probes(script: Vec<Result<(), i32>>) -> (PostRunner, Probes) {
    let seen: Probes = Probes::default();
    let sink = seen.clone();
    let runner = PostRunner::with_runner(Box::new(move |_py, args: &[String]| {
        let out = PathBuf::from(value_of(args, "--json"));
        let model = value_of(args, "--model");
        let step = script[sink.borrow().len().min(script.len() - 1)];
        sink.borrow_mut().push(out.clone());
        match step {
            Ok(()) => {
                std::fs::write(&out, profile_doc(&model)).unwrap();
                Ok(output(exited(0), ""))
            }
            Err(code) => Ok(output(exited(code), &format!("cannot reach model {model}"))),
        }
    }));
    (runner, seen)
}

/// One swap-candidate job's world: a real `Pager` with a real journal, the
/// profiles directory `main.rs` creates with `qwen`'s blessed baseline in it,
/// `qwen` registered as the serving model, and a candidate GGUF on disk.
struct Job {
    jpath: PathBuf,
    pager: std::sync::Mutex<Pager<FakeSubstrate>>,
    store: ProfileStore,
    candidate: PathBuf,
}

/// [`Job`] with the candidate's bytes chosen by the caller — `Some(name)`
/// writes a real GGUF, `None` writes something that is not one.
fn job_with(tag: &str, candidate: Option<&str>) -> Job {
    let dir = scratch(tag);
    let profiles = dir.join("profiles");
    std::fs::create_dir_all(&profiles).expect("profiles dir");
    let jpath = dir.join("j.jsonl");
    let mut pager = Pager::new(
        FakeSubstrate::new(),
        Journal::open(&jpath).expect("journal"),
        ImageStore::new(&dir.join("img")).expect("image store"),
        Box::new(|| Some(10u64.pow(9))),
    );
    let serving = dir.join("serving.gguf");
    write_gguf(&serving, "the-model-in-service");
    pager
        .register_model(
            MODEL,
            &serving,
            parse_gguf_meta(&serving).expect("gguf"),
            None,
        )
        .expect("the configured model registers");
    let candidate_path = dir.join("candidate.gguf");
    match candidate {
        Some(name) => write_gguf(&candidate_path, name),
        None => std::fs::write(&candidate_path, b"this is not a GGUF").unwrap(),
    }
    Job {
        store: ProfileStore::new(&profiles),
        jpath,
        pager: std::sync::Mutex::new(pager),
        candidate: candidate_path,
    }
}

fn job(tag: &str) -> Job {
    job_with(tag, Some("the-candidate"))
}

impl Job {
    /// The blessed baseline the candidate is measured against — spec §4's
    /// floor, which the endpoint's own precondition (Task 3) guarantees exists.
    fn floor(&self) -> PathBuf {
        self.store.paths(MODEL).baseline
    }

    fn seed_floor(&self, doc: &str) {
        std::fs::write(self.floor(), doc).unwrap();
    }

    /// Where the candidate's own profile document is written.
    fn candidate_profile(&self) -> PathBuf {
        self.store.confirm_staging(&scratch_identity(MODEL))
    }

    fn run(
        &self,
        runner: &PostRunner,
        gate: &CoverGate,
        slot: &SwapSlot,
    ) -> Result<(), PagerError> {
        run_candidate_probe(
            &self.pager,
            runner,
            gate,
            &self.store,
            8181,
            &tier(),
            MODEL,
            &self.candidate,
            slot,
        )
    }

    fn events(&self) -> Vec<Event> {
        replay(&self.jpath).unwrap()
    }

    /// Every `SwapCandidate` row, replayed from the journal on disk — so the
    /// row's serialization is exercised by every assertion below, not just its
    /// construction.
    fn swap_rows(&self) -> Vec<Event> {
        self.events()
            .into_iter()
            .filter(|e| matches!(e, Event::SwapCandidate { .. }))
            .collect()
    }

    fn degraded(&self) -> Vec<String> {
        self.events()
            .into_iter()
            .filter_map(|e| match e {
                Event::Degraded { reason } => Some(reason),
                _ => None,
            })
            .collect()
    }

    /// Every model name the pager currently reports — the surface the "never
    /// outlives the job" law is checkable on.
    fn model_names(&self) -> Vec<String> {
        self.pager
            .lock()
            .unwrap()
            .status()
            .models
            .into_iter()
            .map(|m| m.name)
            .collect()
    }

    fn sha_of(&self, path: &Path) -> String {
        sha256_hex_bytes(&std::fs::read(path).expect("bytes to digest"))
    }
}

// ---------------------------------------------------------------------------
// The job — the verdict, journaled with digests (design §4)
// ---------------------------------------------------------------------------

#[test]
fn a_covered_candidate_journals_the_verdict_with_digests() {
    let job = job("covered");
    let floor_doc = profile_doc(MODEL);
    job.seed_floor(&floor_doc);
    let (runner, probes) = scripted_probes(vec![Ok(())]);
    let (gate, calls) = gate_answering(exited(0));
    let slot = SwapSlot::default();
    slot.try_start(MODEL, &job.candidate)
        .expect("the slot is idle");

    job.run(&runner, &gate, &slot)
        .expect("the job records its result");

    let candidate_profile = job.candidate_profile();
    assert_eq!(
        probes.borrow().as_slice(),
        std::slice::from_ref(&candidate_profile),
        "the candidate is probed exactly once, into its own document"
    );
    let spawned: Vec<Vec<String>> = calls
        .borrow()
        .iter()
        .map(|(_, argv)| argv.clone())
        .collect();
    assert_eq!(
        spawned,
        vec![cover_argv(&job.floor(), &candidate_profile)],
        "cover is spawned once, over the floor and the document just probed"
    );

    let events = job.events();
    assert_eq!(events.len(), 1, "one job, one row: {events:?}");
    match &events[0] {
        Event::SwapCandidate {
            model,
            candidate_gguf_sha,
            floor_path,
            floor_sha,
            candidate_profile_path,
            candidate_profile_sha,
            exit_code,
            outcome,
        } => {
            assert_eq!(model, MODEL);
            assert_eq!(outcome, "covered");
            assert_eq!(*exit_code, Some(0));
            assert_eq!(
                *candidate_gguf_sha,
                job.sha_of(&job.candidate),
                "the row's candidate digest is of the candidate GGUF's own bytes"
            );
            assert_eq!(
                *floor_sha,
                sha256_hex_bytes(floor_doc.as_bytes()),
                "the row's floor digest is of the blessed baseline's bytes, so `sha256sum` \
                 on the path it names checks the row"
            );
            assert_eq!(*floor_path, job.floor().display().to_string());
            assert_eq!(
                *candidate_profile_path,
                candidate_profile.display().to_string()
            );
            assert_eq!(
                candidate_profile_sha.as_deref(),
                Some(job.sha_of(&candidate_profile).as_str())
            );
        }
        other => panic!("expected SwapCandidate, got {other:?}"),
    }

    match slot.snapshot() {
        SwapState::Done { model, report } => {
            assert_eq!(model, MODEL);
            assert_eq!(report.outcome, "covered");
            assert_eq!(report.exit_code, Some(0));
            assert_eq!(report.candidate_gguf_sha, job.sha_of(&job.candidate));
            assert_eq!(report.floor_sha, sha256_hex_bytes(floor_doc.as_bytes()));
            assert_eq!(
                report.candidate_profile_path,
                candidate_profile.display().to_string()
            );
            assert_eq!(
                report.notes,
                [NOTE_TASK_GATES, NOTE_HANDOVER],
                "every report names the two gaps §4 requires it to name"
            );
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

/// Spec §7: a probe failure is journaled as degraded and **no verdict is
/// invented**. There is no second document, so there is nothing to compare and
/// no `SwapCandidate` row to write — and cover must never be asked about a
/// document that does not exist.
#[test]
fn a_probe_failure_journals_degraded_and_reports_no_verdict() {
    let job = job("probe-failure");
    job.seed_floor(&profile_doc(MODEL));
    let (runner, _probes) = scripted_probes(vec![Err(4)]);
    let (gate, calls) = gate_answering(exited(0));
    let slot = SwapSlot::default();
    slot.try_start(MODEL, &job.candidate)
        .expect("the slot is idle");

    job.run(&runner, &gate, &slot)
        .expect("the job records its result");

    assert!(
        calls.borrow().is_empty(),
        "cover is never spawned without a candidate document: {:?}",
        calls.borrow()
    );
    assert!(
        job.swap_rows().is_empty(),
        "a failed probe reached no verdict, so it journals no verdict row"
    );
    let degraded = job.degraded();
    assert_eq!(degraded.len(), 1, "{degraded:?}");
    assert!(
        degraded[0].contains(MODEL) && degraded[0].contains("cannot reach model"),
        "the degradation must name the model and the probe's own words: {degraded:?}"
    );

    match slot.snapshot() {
        SwapState::Done { report, .. } => {
            assert!(
                report.outcome.starts_with("infra: "),
                "an infrastructure failure is not a verdict in either direction: {report:?}"
            );
            assert!(
                report.outcome.contains("cannot reach model"),
                "the probe's own words reach the operator without a re-run: {report:?}"
            );
            assert_eq!(
                report.exit_code, None,
                "no cover ran, so there is no exit code — `None`, never 0"
            );
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

/// Spec §4: "The scratch identity never outlives the request." Pinned on both
/// the path that reached a verdict and the path that failed before one.
#[test]
fn the_scratch_registration_never_outlives_the_job() {
    for (tag, script) in [("scratch-ok", Ok(())), ("scratch-failed", Err(4))] {
        let job = job(tag);
        job.seed_floor(&profile_doc(MODEL));
        let (runner, _probes) = scripted_probes(vec![script]);
        let (gate, _calls) = gate_answering(exited(0));
        let slot = SwapSlot::default();

        job.run(&runner, &gate, &slot).expect("the job records");

        assert_eq!(
            job.model_names(),
            vec![MODEL.to_string()],
            "{tag}: the scratch identity is unloaded AND unregistered on every exit path, \
             and the configured model it stood beside is untouched"
        );
    }
}

/// Spec §4: "One candidate at a time … a second request while one runs gets
/// 409 `candidate_probe_in_progress` — no queue."
#[test]
fn the_slot_admits_one_job_at_a_time() {
    let slot = SwapSlot::default();
    assert!(slot.try_start("qwen", Path::new("/a.gguf")).is_ok());
    assert!(
        slot.try_start("qwen", Path::new("/b.gguf")).is_err(),
        "a running job holds the slot against every other claim"
    );

    slot.finish("qwen", finished_report());
    assert!(
        slot.try_start("qwen", Path::new("/b.gguf")).is_ok(),
        "a finished job releases the slot — the bound is one at a time, not one ever"
    );
}

/// A report to hand [`SwapSlot::finish`] in a slot-only test: the slot stores
/// whatever it is given and reads nothing out of it.
fn finished_report() -> bloomery_daemon::swap::SwapOutcomeReport {
    bloomery_daemon::swap::SwapOutcomeReport {
        outcome: "covered".to_string(),
        exit_code: Some(0),
        candidate_gguf_sha: "0".repeat(64),
        floor_sha: "1".repeat(64),
        candidate_profile_path: "/d/candidate.json".to_string(),
        notes: [NOTE_TASK_GATES, NOTE_HANDOVER],
    }
}

/// Each documented code gets its own word, and **a verdict is not an
/// infrastructure failure**: none of the three journals a `Degraded` row.
#[test]
fn refused_incomplete_and_not_covered_all_journal_their_own_word() {
    for (exit, word) in [(1, "not-covered"), (2, "refused"), (3, "incomplete")] {
        let job = job(&format!("verdict-{exit}"));
        job.seed_floor(&profile_doc(MODEL));
        let (runner, _probes) = scripted_probes(vec![Ok(())]);
        let (gate, _calls) = gate_answering(exited(exit));
        let slot = SwapSlot::default();

        job.run(&runner, &gate, &slot).expect("the job records");

        let rows = job.swap_rows();
        assert_eq!(rows.len(), 1, "exit {exit}: {rows:?}");
        match &rows[0] {
            Event::SwapCandidate {
                outcome, exit_code, ..
            } => {
                assert_eq!(outcome, word, "exit {exit} is spelled {word}");
                assert_eq!(*exit_code, Some(exit));
            }
            other => panic!("expected SwapCandidate, got {other:?}"),
        }
        assert!(
            job.degraded().is_empty(),
            "exit {exit} is a verdict, not an infrastructure failure: {:?}",
            job.degraded()
        );
        match slot.snapshot() {
            SwapState::Done { report, .. } => assert_eq!(report.outcome, word),
            other => panic!("expected Done, got {other:?}"),
        }
    }
}

/// The Task-1 review's ruling, carried through to the operator: exit 2 is also
/// what `argparse` returns for `invalid choice: 'cover'`, so a refusal's own
/// words must reach the journal row **and** the report — otherwise a stale
/// assay is indistinguishable from a considered refusal about the candidate,
/// and the operator has to re-run the command to find out which they got.
#[test]
fn a_refusal_carries_assays_words_into_the_row_and_the_report() {
    let job = job("refused-words");
    job.seed_floor(&profile_doc(MODEL));
    let (runner, _probes) = scripted_probes(vec![Ok(())]);
    let (gate, _calls) = gate_saying(exited(2), "invalid choice: 'cover'");
    let slot = SwapSlot::default();

    job.run(&runner, &gate, &slot).expect("the job records");

    let rows = job.swap_rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    match &rows[0] {
        Event::SwapCandidate {
            outcome, exit_code, ..
        } => {
            assert!(
                outcome.starts_with("refused") && outcome.contains("invalid choice: 'cover'"),
                "the row keeps the refusal's word AND assay's reason: {outcome}"
            );
            assert_eq!(*exit_code, Some(2));
        }
        other => panic!("expected SwapCandidate, got {other:?}"),
    }
    match slot.snapshot() {
        SwapState::Done { report, .. } => assert!(
            report.outcome.contains("invalid choice: 'cover'"),
            "the report says why the refusal happened without a re-run: {report:?}"
        ),
        other => panic!("expected Done, got {other:?}"),
    }
}

/// A cover that could not run at all is `infra: …` — still a row, because both
/// documents exist and the comparison was really attempted, unlike the
/// probe-failure path above.
#[test]
fn a_cover_that_cannot_run_is_a_row_naming_the_infrastructure_failure() {
    let job = job("cover-infra");
    job.seed_floor(&profile_doc(MODEL));
    let (runner, _probes) = scripted_probes(vec![Ok(())]);
    let gate = CoverGate::with_runner(Box::new(|_program, _args| {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file or directory",
        ))
    }));
    let slot = SwapSlot::default();

    job.run(&runner, &gate, &slot).expect("the job records");

    let rows = job.swap_rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    match &rows[0] {
        Event::SwapCandidate {
            outcome, exit_code, ..
        } => {
            assert!(
                outcome.starts_with("infra: ") && outcome.contains("no such file"),
                "the failing layer's own words, never a verdict: {outcome}"
            );
            assert_eq!(*exit_code, None);
        }
        other => panic!("expected SwapCandidate, got {other:?}"),
    }
}

/// Every failure named (spec §7), including the ones that happen before
/// anything is registered or probed: a candidate that is not a GGUF, and a
/// floor that cannot be read.
#[test]
fn a_candidate_that_is_not_a_gguf_is_named_and_nothing_is_probed() {
    let job = job_with("bad-gguf", None);
    job.seed_floor(&profile_doc(MODEL));
    let (runner, probes) = scripted_probes(vec![Ok(())]);
    let (gate, calls) = gate_answering(exited(0));
    let slot = SwapSlot::default();

    job.run(&runner, &gate, &slot).expect("the job records");

    assert!(probes.borrow().is_empty(), "nothing is probed");
    assert!(calls.borrow().is_empty(), "nothing is covered");
    assert!(job.swap_rows().is_empty(), "no verdict is invented");
    let degraded = job.degraded();
    assert_eq!(degraded.len(), 1, "{degraded:?}");
    assert!(
        degraded[0].contains("candidate.gguf"),
        "the degradation names the file it could not read: {degraded:?}"
    );
    assert_eq!(
        job.model_names(),
        vec![MODEL.to_string()],
        "a candidate that never registered leaves nothing to clean up"
    );
    match slot.snapshot() {
        SwapState::Done { report, .. } => {
            assert!(report.outcome.starts_with("infra: "), "{report:?}");
            assert_eq!(report.exit_code, None);
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

#[test]
fn a_floor_that_cannot_be_read_is_named_and_nothing_is_probed() {
    let job = job("no-floor"); // no `seed_floor`: the baseline is simply absent
    let (runner, probes) = scripted_probes(vec![Ok(())]);
    let (gate, calls) = gate_answering(exited(0));
    let slot = SwapSlot::default();

    job.run(&runner, &gate, &slot).expect("the job records");

    assert!(probes.borrow().is_empty(), "nothing is probed");
    assert!(calls.borrow().is_empty(), "nothing is covered");
    assert!(job.swap_rows().is_empty(), "no verdict is invented");
    let degraded = job.degraded();
    assert_eq!(degraded.len(), 1, "{degraded:?}");
    assert!(
        degraded[0].contains("baseline"),
        "the degradation names the floor it could not read: {degraded:?}"
    );
}
