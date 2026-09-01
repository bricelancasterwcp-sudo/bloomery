//! Fixtures shared by the `api_native_*` integration tests.
//!
//! These lived in `api_native_test.rs` until that file was split (2026-09-01,
//! carried-debt slice D). They are here rather than duplicated because three
//! of the resulting files need them: `profile_doc` is used by the core route
//! tests, the bless/unblock tests and the swap-candidate tests, and
//! `serve_drift_blocked_qwen` by the core tests and the unblock tests.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bloomery_daemon::config::Tier;
use bloomery_daemon::drift::ProfileStore;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::post::PostRunner;
use bloomery_daemon::swap::{scratch_identity, CoverGate, SwapContext, SwapProbes};
use bloomery_substrate::fake::FakeSubstrate;

use super::http;

/// A minimal but real assay profile document, the same shape `drift_test.rs`
/// and `post_test.rs` use. `max_verified` is the knob that makes two documents
/// genuinely different bytes while still describing the same model measured by
/// the same instrument — which is what a baseline replacement actually meets.
pub fn profile_doc(model: &str, max_verified: u32) -> String {
    format!(
        r#"{{"assay_profile_version":3,"probe_version":"0.4.1","model":{{"name":"{model}"}},"ceiling":{{"max_verified":{max_verified}}},"verdicts":{{}}}}"#
    )
}

/// Builds and serves a `Pager<FakeSubstrate>` with `qwen` registered,
/// profiled (so admission reaches the drift-block clause rather than
/// stopping at `Unprofiled`), and its cumulative drift reading set to a
/// `Confirmed` regression against baseline `"base42"` — the one shape
/// `set_drift` turns into an admission block (Task 2's invariant).
pub fn serve_drift_blocked_qwen() -> (u16, bloomery_daemon::http::ServerHandle) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-drift-blocked-test-{}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");

    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut pager = bloomery_daemon::pager::Pager::new(
        bloomery_substrate::fake::FakeSubstrate::new(),
        journal,
        images,
        Box::new(|| Some(1024 * 1024 * 1024)),
    );
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
    handle.set_scratch_dir(dir);
    (port, handle)
}

// --- swap-candidate fixtures, shared by the two `api_native_swap_*` files ---

/// Every argv one scripted assay was handed, in order. `Arc`/`Mutex` rather
/// than `swap_test.rs`'s `Rc`/`RefCell` because these collaborators are built
/// on the spawned worker thread, not on the test's.
pub type Seen = Arc<Mutex<Vec<Vec<String>>>>;

/// A hook the fake probe runs before it writes its document — how the two
/// failure rows below break the world in the middle of a job. Installed after
/// the fixture exists (the interesting hooks need the fixture's own pager).
pub type Hook = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;

/// Every `/v1/chat/completions` call the fake probe made, as `(model, status)`
/// — the admission the live acceptance found unreachable, observed rather than
/// assumed. Only ever non-empty under [`SwapCfg::drive_v1`].
pub type V1Calls = Arc<Mutex<Vec<(String, u16)>>>;

/// The configured model whose role every candidate below would take.
pub const SWAP_MODEL: &str = "qwen";

/// A daemon serving `qwen` with the swap-candidate surface wired: the profiles
/// directory `main.rs` wires, a real candidate GGUF on disk, and both of the
/// job's subprocesses scripted — the probe writing a real document for
/// whatever `--model` it is handed, `cover` answering `cover_exit`.
pub struct SwapFixture {
    pub port: u16,
    pub handle: bloomery_daemon::http::ServerHandle,
    pub dir: PathBuf,
    pub pager: Arc<Mutex<Pager<FakeSubstrate>>>,
    pub ctx: Arc<SwapContext>,
    pub candidate: PathBuf,
    pub probes: Seen,
    pub covers: Seen,
    pub hook: Hook,
    /// Fires inside the fake `cover`, i.e. strictly AFTER the probe step and
    /// while the scratch identity is still registered — the one moment a test
    /// can ask whether the probe's admission window is still open.
    pub cover_hook: Hook,
    pub v1: V1Calls,
}

/// How one fixture's daemon differs from the default. Both knobs exist for the
/// admission rows at the end of this file, and both default to the shape every
/// earlier row was written against.
#[derive(Clone, Copy)]
pub struct SwapCfg {
    /// `false` is the standing production config: `allow_unprofiled` unset, so
    /// law 5's gate really refuses and the candidate probe's own `/v1` call is
    /// admitted by the candidate window or not at all. The fixture's default
    /// `true` is `Pager::new`'s permissive default, which every earlier row in
    /// this file was written against.
    pub allow_unprofiled: bool,
    /// The fake probe first drives this daemon's real `/v1/chat/completions`
    /// under the identity it was handed, exactly as assay does, and reports a
    /// non-200 the way assay reported the live 422 — `exit 4`, no document.
    /// This is the whole point of the admission rows: with it off, the probe
    /// seam is scripted end to end and real admission never runs.
    pub drive_v1: bool,
    /// The fake probe exits 4 without writing a document *after* its `/v1`
    /// call — one job's probe-failure path, driven without touching admission,
    /// so a row can watch the window open and the job still end badly.
    pub probe_fails: bool,
}

impl Default for SwapCfg {
    fn default() -> Self {
        SwapCfg {
            allow_unprofiled: true,
            drive_v1: false,
            probe_fails: false,
        }
    }
}

pub fn serve_swap(cover_exit: i32) -> SwapFixture {
    serve_swap_cfg(cover_exit, SwapCfg::default())
}

pub fn serve_swap_cfg(cover_exit: i32, cfg: SwapCfg) -> SwapFixture {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("bloomery-swap-http-{}-{seq}", std::process::id()));
    let profiles = dir.join("profiles");
    std::fs::create_dir_all(&profiles).expect("scratch dir");

    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    // Enough replies that every `/v1` call a probe makes is answered like a
    // real one: admission is what these rows measure, so an inference that
    // failed for want of a script would be noise in the status they read.
    for _ in 0..16 {
        fake.script_reply(bloomery_substrate::Reply {
            text: "ok".to_string(),
            prompt_tokens: Some(4),
            completion_tokens: Some(2),
            duration_ms: 1,
        });
    }
    let mut pager = Pager::new(fake, journal, images, Box::new(|| Some(1024 * 1024 * 1024)));
    pager.set_allow_unprofiled(cfg.allow_unprofiled);
    pager.set_profiles_dir(profiles.clone());
    let serving = dir.join("qwen.gguf");
    write_gguf(&serving, "the-model-in-service");
    pager
        .register_model(
            SWAP_MODEL,
            &serving,
            bloomery_core::gguf::parse_gguf_meta(&serving).expect("gguf"),
            None,
        )
        .unwrap();
    let candidate = dir.join("candidate.gguf");
    write_gguf(&candidate, "the-candidate");

    let probes: Seen = Seen::default();
    let covers: Seen = Seen::default();
    let hook: Hook = Hook::default();
    let cover_hook: Hook = Hook::default();
    let v1: V1Calls = V1Calls::default();
    let (probe_sink, cover_sink, probe_hook) = (probes.clone(), covers.clone(), hook.clone());
    let (v1_sink, after_probe) = (v1.clone(), cover_hook.clone());
    let factory = Box::new(move || {
        let (sink, hook, v1_sink) = (probe_sink.clone(), probe_hook.clone(), v1_sink.clone());
        let drive_v1 = cfg.drive_v1;
        let runner = PostRunner::with_runner(Box::new(move |_py, args: &[String]| {
            sink.lock().expect("probe sink").push(args.to_vec());
            let model = value_of(args, "--model");
            if drive_v1 {
                // assay's real first act: drive the endpoint it was pointed at,
                // under the `--model` it was handed. The base URL comes out of
                // the argv rather than a captured port, because the argv is
                // what the job really built.
                let base = args
                    .iter()
                    .find(|a| a.starts_with("http://"))
                    .cloned()
                    .expect("the probe argv names an endpoint");
                let (addr, path) = base
                    .trim_start_matches("http://")
                    .split_once('/')
                    .expect("the endpoint carries assay's /v1 suffix");
                let (st, _) = http(
                    addr,
                    "POST",
                    &format!("/{path}/chat/completions"),
                    &serde_json::json!({
                        "model": model,
                        "messages": [{"role": "user", "content": "hi"}],
                        "max_tokens": 8,
                    })
                    .to_string(),
                );
                v1_sink.lock().expect("v1 sink").push((model.clone(), st));
                if st != 200 {
                    // The live run's own words, in the live run's own shape:
                    // `PostError::NonZeroExit` renders this as `assay exited 4:
                    // …`, which is exactly what the endpoint reported twice
                    // against the real daemon.
                    return Ok(std::process::Output {
                        status: exited(4),
                        stdout: Vec::new(),
                        stderr: format!(
                            "assay: infrastructure failure: HTTP {st} from {base}/chat/completions"
                        )
                        .into_bytes(),
                    });
                }
            }
            // Cloned out from under the lock, so a hook that panics (the
            // wedge row below) cannot poison this mutex on the way past.
            let installed = hook.lock().expect("probe hook").clone();
            if let Some(f) = installed {
                f();
            }
            if cfg.probe_fails {
                return Ok(std::process::Output {
                    status: exited(4),
                    stdout: Vec::new(),
                    stderr: b"assay: scripted probe failure".to_vec(),
                });
            }
            let out = PathBuf::from(value_of(args, "--json"));
            std::fs::write(&out, profile_doc(&model, 2048)).expect("fake probe writes a document");
            Ok(output(exited(0)))
        }));
        let (sink, hook) = (cover_sink.clone(), after_probe.clone());
        let gate = CoverGate::with_runner(Box::new(move |_py, args: &[String]| {
            sink.lock().expect("cover sink").push(args.to_vec());
            let installed = hook.lock().expect("cover hook").clone();
            if let Some(f) = installed {
                f();
            }
            Ok(output(exited(cover_exit)))
        }));
        SwapProbes { runner, gate }
    });

    let pager = Arc::new(Mutex::new(pager));
    let ctx = Arc::new(SwapContext::with_probes(
        factory,
        ProfileStore::new(&profiles),
        Tier {
            name: "enthusiast-16gb".into(),
            emulated: false,
        },
    ));
    let (port, mut handle) =
        bloomery_daemon::http::serve_shared_with_swap(Arc::clone(&pager), 0, Arc::clone(&ctx));
    handle.set_scratch_dir(dir.clone());
    SwapFixture {
        port,
        handle,
        dir,
        pager,
        ctx,
        candidate,
        probes,
        covers,
        hook,
        cover_hook,
        v1,
    }
}

impl SwapFixture {
    pub fn addr(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// The blessed baseline the endpoint requires before it will probe
    /// anything — the operator-endorsed capability statement, never the
    /// merely-latest profile.
    pub fn seed_floor(&self) {
        std::fs::write(
            self.dir
                .join("profiles")
                .join(format!("{SWAP_MODEL}.baseline.json")),
            profile_doc(SWAP_MODEL, 2048),
        )
        .unwrap();
    }

    pub fn body(&self) -> String {
        serde_json::json!({"gguf_path": self.candidate.display().to_string()}).to_string()
    }

    pub fn post(&self, body: &str) -> (u16, String) {
        http(
            &self.addr(),
            "POST",
            &format!("/models/{SWAP_MODEL}/swap-candidate"),
            body,
        )
    }

    pub fn get(&self) -> (u16, String) {
        http(
            &self.addr(),
            "GET",
            &format!("/models/{SWAP_MODEL}/swap-candidate"),
            "",
        )
    }

    /// Polls `GET` until the job leaves `running`, bounded exactly like every
    /// other poll loop in this crate's tests (200 × 20 ms).
    pub fn poll_until_done(&self) -> serde_json::Value {
        let mut last = String::new();
        for _ in 0..200 {
            let (st, body) = self.get();
            assert_eq!(st, 200, "{body}");
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            if v["state"] != "running" {
                return v;
            }
            last = body;
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("the candidate job never left `running`: {last}");
    }

    /// One `/v1/chat/completions` against this daemon under `model` — the
    /// request the candidate probe makes, made by hand so a test can ask the
    /// admission question at a moment of its own choosing.
    pub fn chat(&self, model: &str) -> (u16, String) {
        http(
            &self.addr(),
            "POST",
            "/v1/chat/completions",
            &serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 8,
            })
            .to_string(),
        )
    }

    /// Whether the candidate probe's admission window is still open on the
    /// scratch identity — the window state itself, read off the pager, rather
    /// than inferred from what `/v1` happened to answer.
    pub fn window_open(&self) -> bool {
        self.pager
            .lock()
            .expect("the pager survives every candidate job")
            .probe_window_open(&scratch_identity(SWAP_MODEL))
    }

    pub fn events(&self) -> Vec<bloomery_core::journal::Event> {
        bloomery_core::journal::replay(&self.dir.join("j.jsonl")).unwrap()
    }

    pub fn degraded_reasons(&self) -> Vec<String> {
        self.events()
            .iter()
            .filter_map(|e| match e {
                bloomery_core::journal::Event::Degraded { reason } => Some(reason.clone()),
                _ => None,
            })
            .collect()
    }
}

// --- minimal GGUF writer, used by the swap fixtures above ---

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

/// A **real, parseable** GGUF file, copied from `swap_test.rs`: the worker
/// registers the candidate through `parse_gguf_meta` + `register_model`, the
/// same pair `main.rs` registers every configured model with, so a placeholder
/// byte string would never get past the registration.
pub fn write_gguf(path: &Path, name: &str) {
    use std::io::Write;
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

/// A wait status carrying exit code `code` — the encoding `waitpid` returns.
/// Copied from `swap_test.rs` rather than shared: each file under `tests/` is
/// its own crate, and this is three lines.
pub fn exited(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code << 8)
}

/// The value following `flag` in an argv, or the empty string.
pub fn value_of(args: &[String], flag: &str) -> String {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default()
}

pub fn output(status: std::process::ExitStatus) -> std::process::Output {
    std::process::Output {
        status,
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}
