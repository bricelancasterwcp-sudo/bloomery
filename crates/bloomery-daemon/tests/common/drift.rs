//! Fixtures shared by the `drift_*` integration tests.
//!
//! These lived in `drift_test.rs` until it was split (2026-09-01, carried-debt
//! slice D). They are here rather than duplicated because every one of them is
//! reached by at least two of the resulting files -- `profile_doc` and `boot`
//! by three apiece -- which is exactly why the original file resisted being
//! split by section alone.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bloomery_core::journal::{replay, sha256_hex_bytes, Event, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::Tier;
use bloomery_daemon::drift::{DriftGate, ModelDrift, ProfileStore};
use bloomery_daemon::pager::Pager;
use bloomery_daemon::post::PostRunner;
use bloomery_substrate::fake::FakeSubstrate;

/// Where a fake assay was asked to write, once per probe, in order. The
/// *length* is the load-bearing part: confirm-then-alarm is a claim about how
/// many times a model is probed in one boot.
pub type Probes = std::rc::Rc<std::cell::RefCell<Vec<PathBuf>>>;

/// A scratch dir plus the `profiles/` subdirectory `main.rs` creates, and a
/// store rooted at it.
pub fn store_in(tag: &str) -> (PathBuf, PathBuf, ProfileStore) {
    let dir = scratch(tag);
    let profiles = dir.join("profiles");
    std::fs::create_dir_all(&profiles).expect("profiles dir");
    let store = ProfileStore::new(&profiles);
    (dir, profiles, store)
}

/// A minimal but real assay profile document — the same shape `post_test.rs`
/// feeds its fake assay, so what parses here parses there.
pub fn profile_doc(model: &str) -> String {
    profile_doc_ceiling(model, 2048)
}

/// [`profile_doc`] with the ceiling as a knob, so a test can put two
/// *different* documents on disk that are still the same model measured by the
/// same instrument — which is what a drift comparison actually meets, and what
/// makes "which document is the reference" a checkable claim rather than a
/// tautology.
pub fn profile_doc_ceiling(model: &str, max_verified: u32) -> String {
    format!(
        r#"{{"assay_profile_version":3,"probe_version":"0.4.1","model":{{"name":"{model}"}},"ceiling":{{"max_verified":{max_verified}}},"verdicts":{{}}}}"#
    )
}

pub fn set_mtime(path: &Path, t: SystemTime) {
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for set_times");
    f.set_times(std::fs::FileTimes::new().set_modified(t))
        .expect("set mtime");
}

pub const V8_QWEN3_8B: &str = include_str!("../fixtures/profile-v8-qwen3-8b.json");

pub const V4_QWEN3_8B: &str = include_str!("../fixtures/profile-v4-qwen3-8b.json");

/// A wait status carrying exit code `code` — the encoding `waitpid` returns.
pub fn exited(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code << 8)
}

pub fn qwen_like_meta() -> bloomery_core::gguf::GgufMeta {
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

pub fn tier() -> Tier {
    Tier {
        name: "enthusiast-16gb".into(),
        emulated: false,
    }
}

/// A fake assay whose Nth probe follows `script[N]` (the last entry repeats):
/// `Ok(doc)` writes that document to the `--json` path it was handed and exits
/// 0; `Err(code)` exits `code` having written nothing, exactly as a failing
/// probe does.
pub fn scripted_probes(script: Vec<Result<String, i32>>) -> (PostRunner, Probes) {
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
pub fn gate_deciding(
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

pub fn boot(tag: &str) -> Boot {
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
pub fn boot_for(tag: &str, model: &str) -> Boot {
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

pub fn sha8(doc: &str) -> String {
    sha256_hex_bytes(doc.as_bytes())[..8].to_string()
}

/// A scratch directory unique to this process and this call, so the
/// integration binary's concurrently-running tests never share one.
pub fn scratch(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("bloomery-drift-{}-{seq}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

pub fn value_of(args: &[String], flag: &str) -> String {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default()
}

pub fn output(status: std::process::ExitStatus, stderr: &str) -> std::process::Output {
    std::process::Output {
        status,
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

/// Every `(program, argv)` a gate spawned, in order. `Rc`/`RefCell` because a
/// [`bloomery_daemon::post::CommandRunner`] is deliberately not `Send` and
/// these tests are single-threaded.
pub type Calls = std::rc::Rc<std::cell::RefCell<Vec<(String, Vec<String>)>>>;

/// One model's whole boot: a real `Pager` with a real journal, the profiles
/// directory `main.rs` creates, and `qwen` registered but unprofiled — the
/// state `run_post` actually meets.
pub struct Boot {
    pub profiles: PathBuf,
    pub jpath: PathBuf,
    pub pager: std::sync::Mutex<Pager<FakeSubstrate>>,
    pub model: String,
}

impl Boot {
    /// Writes a document into the profiles directory as if an earlier boot (or
    /// an operator's blessing) had left it there.
    pub fn seed(&self, name: &str, doc: &str) {
        std::fs::write(self.profiles.join(name), doc).unwrap();
    }

    pub fn run(&self, runner: &PostRunner, gate: &DriftGate) {
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

    pub fn events(&self) -> Vec<Event> {
        replay(&self.jpath).unwrap()
    }

    /// Every drift row as `(comparison, outcome, current_path)` — the three
    /// fields the orchestration's claims are about.
    pub fn drift_rows(&self) -> Vec<(String, String, String)> {
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

    pub fn drift(&self) -> Option<ModelDrift> {
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

    pub fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.profiles.join(name)).unwrap_or_else(|e| {
            panic!("expected {name} in the profiles directory: {e}");
        })
    }

    pub fn transients(&self) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(&self.profiles)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| is_transient(&p.display().to_string()))
            .collect();
        found.sort();
        found
    }
}

/// True for a confirm run's document: retention names it by content, and that
/// name is how a test (and an operator reading the journal) tells the confirm
/// re-diff from the first reading.
pub fn is_transient(path: &str) -> bool {
    path.contains(".transient-")
}
