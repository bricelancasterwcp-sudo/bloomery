//! Fixtures shared by the `swap_job_*` tests (design §4's job harness).
//!
//! Split out on 2026-09-01 (carried-debt slice D).

use bloomery_core::gguf::parse_gguf_meta;
use bloomery_core::journal::{replay, sha256_hex_bytes, Event, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::Tier;
use bloomery_daemon::drift::ProfileStore;
use bloomery_daemon::pager::{Pager, PagerError};
use bloomery_daemon::post::PostRunner;
use bloomery_daemon::swap::{run_candidate_probe, scratch_identity, CoverGate, SwapSlot};
use bloomery_substrate::fake::FakeSubstrate;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::swap::{exited, output};

/// The configured model whose role the candidate would take.
pub const MODEL: &str = "qwen";

/// A scratch directory unique to this process and this call, so the
/// integration binary's concurrently-running tests never share one.
pub fn scratch(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("bloomery-swap-{}-{seq}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// The ceiling every fixture document reports unless a test needs two
/// documents whose **bytes** differ.
pub const DEFAULT_CEILING: u32 = 2048;

/// A minimal but real assay profile document — the same shape `drift_test.rs`
/// and `post_test.rs` feed their fake assay, so what parses there parses here.
pub fn profile_doc(model: &str) -> String {
    profile_doc_measuring(model, DEFAULT_CEILING)
}

/// The same document with its one non-identity number chosen by the caller.
/// Two jobs whose documents differ in **bytes** retain under two different
/// content names, which is what the retention pin needs — and they still agree
/// on identity, which is what keeps `cover` willing to read them.
pub fn profile_doc_measuring(model: &str, max_verified: u32) -> String {
    format!(
        r#"{{"assay_profile_version":3,"probe_version":"0.4.1","model":{{"name":"{model}"}},"ceiling":{{"max_verified":{max_verified}}},"verdicts":{{}}}}"#
    )
}

pub fn tier() -> Tier {
    Tier {
        name: "enthusiast-16gb".into(),
        emulated: false,
    }
}

pub fn kv_string(buf: &mut Vec<u8>, key: &str, val: &str) {
    buf.extend((key.len() as u64).to_le_bytes());
    buf.extend(key.as_bytes());
    buf.extend(8u32.to_le_bytes());
    buf.extend((val.len() as u64).to_le_bytes());
    buf.extend(val.as_bytes());
}

pub fn kv_u32(buf: &mut Vec<u8>, key: &str, val: u32) {
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
pub fn write_gguf(path: &Path, name: &str) {
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
pub type Probes = std::rc::Rc<std::cell::RefCell<Vec<PathBuf>>>;

pub fn value_of(args: &[String], flag: &str) -> String {
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
pub fn scripted_probes(script: Vec<Result<(), i32>>) -> (PostRunner, Probes) {
    scripted_probes_measuring(script, DEFAULT_CEILING)
}

/// [`scripted_probes`] whose written document reports `max_verified`, so two
/// jobs in one test write bytes that differ.
pub fn scripted_probes_measuring(
    script: Vec<Result<(), i32>>,
    max_verified: u32,
) -> (PostRunner, Probes) {
    let seen: Probes = Probes::default();
    let sink = seen.clone();
    let runner = PostRunner::with_runner(Box::new(move |_py, args: &[String]| {
        let out = PathBuf::from(value_of(args, "--json"));
        let model = value_of(args, "--model");
        let step = script[sink.borrow().len().min(script.len() - 1)];
        sink.borrow_mut().push(out.clone());
        match step {
            Ok(()) => {
                std::fs::write(&out, profile_doc_measuring(&model, max_verified)).unwrap();
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
pub struct Job {
    pub jpath: PathBuf,
    pub pager: std::sync::Mutex<Pager<FakeSubstrate>>,
    pub store: ProfileStore,
    pub candidate: PathBuf,
}

/// [`Job`] with the candidate's bytes chosen by the caller — `Some(name)`
/// writes a real GGUF, `None` writes something that is not one.
pub fn job_with(tag: &str, candidate: Option<&str>) -> Job {
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

pub fn job(tag: &str) -> Job {
    job_with(tag, Some("the-candidate"))
}

impl Job {
    /// The blessed baseline the candidate is measured against — spec §4's
    /// floor, which the endpoint's own precondition (Task 3) guarantees exists.
    pub fn floor(&self) -> PathBuf {
        self.store.paths(MODEL).baseline
    }

    pub fn seed_floor(&self, doc: &str) {
        std::fs::write(self.floor(), doc).unwrap();
    }

    /// The one fixed path the candidate's probe writes to — POST's own
    /// delete-before-probe path, shared by every candidate ever offered for
    /// this model, which is exactly why the document does not stay here.
    pub fn staging(&self) -> PathBuf {
        self.store.confirm_staging(&scratch_identity(MODEL))
    }

    /// The content-named documents retained for the candidate identity
    /// (design §4 step 3: `{scratch}.transient-{sha8}.json`, beside the drift
    /// transients), sorted. Found by reading the directory rather than by
    /// re-deriving the name, so what this returns is what is really on disk.
    pub fn retained_candidates(&self) -> Vec<PathBuf> {
        let prefix = format!("{}.transient-", scratch_identity(MODEL));
        let mut found: Vec<PathBuf> = std::fs::read_dir(self.store.root())
            .expect("profiles dir")
            .map(|entry| entry.expect("dir entry").path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".json"))
            })
            .collect();
        found.sort();
        found
    }

    pub fn run(
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

    pub fn events(&self) -> Vec<Event> {
        replay(&self.jpath).unwrap()
    }

    /// Every `SwapCandidate` row, replayed from the journal on disk — so the
    /// row's serialization is exercised by every assertion below, not just its
    /// construction.
    pub fn swap_rows(&self) -> Vec<Event> {
        self.events()
            .into_iter()
            .filter(|e| matches!(e, Event::SwapCandidate { .. }))
            .collect()
    }

    pub fn degraded(&self) -> Vec<String> {
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
    pub fn model_names(&self) -> Vec<String> {
        self.pager
            .lock()
            .unwrap()
            .status()
            .models
            .into_iter()
            .map(|m| m.name)
            .collect()
    }

    pub fn sha_of(&self, path: &Path) -> String {
        sha256_hex_bytes(&std::fs::read(path).expect("bytes to digest"))
    }
}
