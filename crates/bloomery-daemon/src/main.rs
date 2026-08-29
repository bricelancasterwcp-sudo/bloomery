//! Daemon entry point.
//!
//! Boots (parse `--config`, load it, open the boot journal, record
//! `Event::Boot`), then hands off to `run`. `run` is feature-gated: the real
//! substrate is llama.cpp (Task 11), compiled only under the `llama` feature
//! so the rest of the workspace — build and test suite alike — stays
//! GPU-free and toolchain-free (see `bloomery-substrate/Cargo.toml`).
//! Without that feature there is no substrate to serve requests against, so
//! the daemon refuses to start with a named reason rather than silently
//! serving a `Pager` that can never load a model.
//!
//! **`LlamaSubstrate` itself is not `Send`** (`LlamaContext` holds raw FFI
//! pointers — `NonNull<llama_context>`, `*mut llama_sampler` — both
//! conservatively `!Send`), which would otherwise block `Pager<S>` from
//! moving into `http::serve`'s `Arc<Mutex<_>>` (`S: Substrate + Send +
//! 'static`, fixed by the Task 14 brief). The `llama` build below serves
//! `Pager<llama_send::SendLlama>` instead: a daemon-owned newtype that
//! delegates every `Substrate` method to `LlamaSubstrate` and asserts
//! `Send` (never `Sync`) under a documented safety argument — see
//! `llama_send`'s module doc for the full justification. That keeps the
//! soundness obligation next to the `Mutex` that actually discharges it,
//! rather than folding it into `bloomery-substrate`'s own contract.

use std::path::PathBuf;

use bloomery_core::journal::{Event, Journal};
use bloomery_daemon::config::{load_config, Config};

fn parse_config_path(args: &[String]) -> Result<PathBuf, String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--config" {
            return iter
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "--config requires a path argument".to_string());
        }
    }
    Err("missing required --config <path> argument".to_string())
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("bloomery-daemon: {msg}");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config_path = parse_config_path(&args).unwrap_or_else(|e| fail(e));

    let config = load_config(&config_path).unwrap_or_else(|e| fail(format!("config: {e}")));

    let journal_dir = config.data_dir.join("journal");
    std::fs::create_dir_all(&journal_dir)
        .unwrap_or_else(|e| fail(format!("failed to create journal dir: {e}")));

    let boot_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let journal_path = journal_dir.join(format!("boot-{boot_ts}.jsonl"));

    let mut journal = Journal::open(&journal_path)
        .unwrap_or_else(|e| fail(format!("failed to open journal: {e}")));

    journal
        .append(&Event::Boot {
            version: env!("CARGO_PKG_VERSION").to_string(),
        })
        .unwrap_or_else(|e| fail(format!("failed to append boot event: {e}")));

    run(config, journal);
}

/// Builds a real `Pager<SendLlama>` from the operator's config, serves it,
/// and runs POST against it.
///
/// **Boot order matters and is not arbitrary** (the chicken-and-egg the
/// spec's §4.7 resolves — see `bloomery_daemon::post` for the argument):
///
/// 1. Wire policy the pager has no opinion about — overhead, request
///    defaults, `allow_unprofiled`, the declared tier.
/// 2. Register every configured model **unprofiled**. A profile can only be
///    produced by probing a *serving* daemon, so at this point none exists.
/// 3. Open the provisional-admission window (`posting`) *before* the socket
///    binds, so nothing can arrive between bind and flag.
/// 4. Serve.
/// 5. Run POST on its own thread — assay talks to this daemon's own `/v1`
///    surface, so the accept loop must be answering while it works — which
///    attaches what it measures and closes the window when it finishes.
/// 6. On that same thread, strictly after POST returns `Ok`, run the G4
///    codec probe (`codec_probe::run_boot_codec_probe`) when
///    `tasks_enabled` says there is a mutating-verb surface worth gating —
///    see that module's docs for the full boot decision table.
#[cfg(feature = "llama")]
fn run(config: Config, journal: Journal) -> ! {
    use std::process::Command;
    use std::sync::{Arc, Mutex};

    use bloomery_core::gguf::parse_gguf_meta;
    use bloomery_core::vram::free_vram_bytes;
    use bloomery_daemon::agents::ImageStore;
    use bloomery_daemon::codec_probe::{
        run_boot_codec_probe, run_boot_g5_probe, should_run_codec_probe,
        G5_POST_DISABLED_SKIP_REASON, POST_DISABLED_CODEC_SKIP_REASON,
    };
    use bloomery_daemon::drift::ProfileStore;
    use bloomery_daemon::http::serve_shared_with_swap_and_memory;
    use bloomery_daemon::llama_send::SendLlama;
    use bloomery_daemon::memory::build_memory;
    use bloomery_daemon::pager::Pager;
    use bloomery_daemon::post::{run_post, PostRunner};
    use bloomery_daemon::swap::SwapContext;

    let substrate = SendLlama::new()
        .unwrap_or_else(|e| fail(format!("failed to initialize the llama backend: {e:?}")));

    let images_dir = config.data_dir.join("images");
    let images = ImageStore::new(&images_dir)
        .unwrap_or_else(|e| fail(format!("failed to open image store: {e}")));

    // One-shot boot-time read: the `free_vram` closure `Pager::new` takes is
    // a STATIC budget, measured once and closed over, never a live
    // per-placement probe (Task 13's pinned convention — a live read would
    // already exclude this pager's own residents and double-count them).
    let probe = free_vram_bytes(|cmd, args| {
        let output = Command::new(cmd).args(args).output()?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    });
    if probe.is_none() {
        eprintln!(
            "bloomery-daemon: warning: could not measure free VRAM (nvidia-smi missing or its \
             output was unparseable); residency will be capped at one resident agent"
        );
    }

    let mut pager = Pager::new(substrate, journal, images, Box::new(move || probe));
    // Binding obligation carried from Task 13: `Pager`'s own default
    // overhead is zero, and this is the daemon's real construction path, so
    // it must wire the operator's configured overhead itself. The same
    // applies to every other policy default below — the pager ships
    // permissive/zero values precisely so this one site is the only place
    // an operator's config takes effect.
    pager.set_overhead_bytes(config.overhead_mib.saturating_mul(1024 * 1024));
    pager.set_ctx_overhead_bytes(config.ctx_overhead_mib.saturating_mul(1024 * 1024));
    pager.set_defaults(config.default_priority, config.default_budget_tokens);
    pager.set_allow_unprofiled(config.allow_unprofiled);
    pager.set_tier(&config.tier.name, config.tier.emulated);
    pager.set_time_share_quantum_ms(config.time_share_quantum_secs.saturating_mul(1000));
    pager.set_tasks_enabled(config.tasks_enabled);
    pager.set_exec_bounds(bloomery_daemon::task::ExecBounds {
        read_cap_bytes: config.read_cap_bytes,
        find_result_cap: config.find_result_cap,
        run_output_cap_bytes: config.run_output_cap_bytes,
        run_timeout_secs: config.run_timeout_secs,
    });
    // A single shared file every task run appends to, distinct from the
    // boot journal above (`journal_path`) — see `task::registry`'s module
    // docs for why a fresh `Journal` handle per task run is safe against
    // this one file specifically.
    pager.set_task_journal_path(config.data_dir.join("journal").join("tasks.jsonl"));
    // The drift watch's filing cabinet (drift-watch design §5). Wired onto the
    // pager because the operator bless route (`POST /models/{name}/bless`) runs
    // on a request thread whose only handle on this daemon is the shared pager;
    // the boot POST below is handed the very same path, so the two cannot come
    // to disagree about where a model's profiles live. Set unconditionally —
    // with `assay.enabled = false` nothing ever writes a current profile there,
    // and the bless route answers that state by name rather than by pointing at
    // a directory this daemon was never told about.
    let profiles_dir = config.data_dir.join("profiles");
    pager.set_profiles_dir(profiles_dir.clone());

    // Unprofiled on purpose: POST is the only source of a profile, and it
    // cannot run until this daemon is serving.
    for (name, spec) in &config.models {
        let path = spec.path();
        let meta = parse_gguf_meta(path).unwrap_or_else(|e| {
            fail(format!(
                "model {name}: could not read {}: {e}",
                path.display()
            ))
        });
        pager
            .register_model(name, path, meta, None)
            .unwrap_or_else(|e| fail(format!("model {name}: {e}")));
        // Task 3, spec §2/§3/§4: per-model tuning from the config's `Tuned`
        // shape. MiB->bytes conversion happens HERE — the pager speaks
        // bytes only, same as `overhead_mib`/`ctx_overhead_mib` just above.
        // `None` for either field is "no override": today's full-charge,
        // full-offload behavior.
        let weights_vram_bytes = spec
            .weights_vram_mib()
            .map(|mib| mib.saturating_mul(1024 * 1024));
        pager
            .set_model_tuning(name, spec.n_gpu_layers(), weights_vram_bytes)
            .unwrap_or_else(|e| fail(format!("model {name}: {e}")));
        // Amendments 2/3 (docs/superpowers/evidence/2026-08-15-g4-protocol.md
        // §10/§11): an explicit per-model operator choice, wired the same
        // way as every other tuning field above — `main.rs` is the one
        // place a config value takes effect. `load_config` already
        // validated every model's envelope (config.rs), so this `Result` is
        // never actually `Err` here — handled the same uniform way as this
        // loop's other accessors rather than an `.expect` that would read as
        // a special case.
        let envelope = spec
            .envelope_lens()
            .unwrap_or_else(|e| fail(format!("model {name}: {e}")));
        pager
            .set_model_envelope(name, envelope)
            .unwrap_or_else(|e| fail(format!("model {name}: {e}")));
        // Spec §10 addendum (declared KV-per-token override): same wiring
        // shape as `weights_vram_mib` just above, but no MiB->bytes
        // conversion — `kv_per_token_bytes` is already declared in bytes
        // (spec §10: "Per-model config `kv_per_token_bytes = N` (bytes)").
        pager
            .set_kv_per_token_bytes(name, spec.kv_per_token_bytes())
            .unwrap_or_else(|e| fail(format!("model {name}: {e}")));
    }

    // Opened before the socket binds, so no request can arrive in the gap
    // between "serving" and "provisionally admitting". `run_post` closes it.
    if config.assay.enabled {
        pager.set_posting(true);
    }

    let pager = Arc::new(Mutex::new(pager));
    // The swap-candidate seam's collaborators (design §4), wired
    // unconditionally for the same reason `set_profiles_dir` above is: the
    // route runs on a request thread, and with `assay.enabled = false` nothing
    // ever blesses a baseline, so the endpoint answers `no_baseline` by name
    // rather than pointing at machinery this daemon was never told about. The
    // interpreter and the probe cap are POST's own, so a candidate is measured
    // by exactly the instrument every configured model was.
    let swap = Arc::new(SwapContext::new(
        config.assay.python.clone(),
        std::time::Duration::from_secs(config.assay.probe_timeout_secs),
        ProfileStore::new(profiles_dir.clone()),
        config.tier.clone(),
    ));
    // Task 8, memory-organ design §6/§7: built once, at boot, from the
    // operator's `[memory]` config — a load failure degrades to
    // `disabled_reason` rather than blocking boot, so this call never fails.
    let memory = build_memory(&config.memory, &config.data_dir);
    let (bound_port, _handle) =
        serve_shared_with_swap_and_memory(Arc::clone(&pager), config.port, swap, memory);
    println!(
        "bloomery-daemon serving on 127.0.0.1:{bound_port} (data_dir={}; models: {}; tier: {} \
         {}; POST: {})",
        config.data_dir.display(),
        config.models.len(),
        config.tier.name,
        if config.tier.emulated {
            "emulated"
        } else {
            "real-hardware"
        },
        if config.assay.enabled {
            "running"
        } else {
            "disabled"
        },
    );

    if config.assay.enabled {
        std::fs::create_dir_all(&profiles_dir)
            .unwrap_or_else(|e| fail(format!("failed to create profiles dir: {e}")));
        let models: Vec<String> = config.models.keys().cloned().collect();
        // G5 (docs/superpowers/evidence/2026-08-16-g5-protocol.md §1):
        // per-model opt-in, computed the same way `models` above is — every
        // configured name whose spec has `g5_probe = true`.
        // Each opted-in model travels with its envelope lens (turn-6 spec
        // §5.1): the G5 probe selects the instrument per envelope. An
        // unparseable envelope value is a config error and fails the boot
        // here, exactly as the pager's own registration path would.
        let g5_models: Vec<(String, bloomery_daemon::config::EnvelopeLens)> = config
            .models
            .iter()
            .filter(|(_, spec)| spec.g5_probe())
            .map(|(name, spec)| {
                let lens = spec
                    .envelope_lens()
                    .unwrap_or_else(|e| fail(format!("model {name:?}: {e}")));
                (name.clone(), lens)
            })
            .collect();
        let tier = config.tier.clone();
        let python = config.assay.python.clone();
        let post_pager = Arc::clone(&pager);
        let tasks_enabled = config.tasks_enabled;
        let codec_scratch_dir = config.data_dir.join("codec-probe");
        let probe_timeout = std::time::Duration::from_secs(config.assay.probe_timeout_secs);
        std::thread::spawn(move || {
            let runner = PostRunner::new(python, probe_timeout);
            match run_post(
                &post_pager,
                &runner,
                &models,
                bound_port,
                &tier,
                &profiles_dir,
            ) {
                Ok(()) => {
                    // Strictly after `run_post`: every model's profile is
                    // attached and `posting` has cleared, so the probe
                    // measures under exactly the codec every other request
                    // would dispatch under (protocol §4). `assay.enabled` is
                    // `true` here by construction (this is that branch).
                    if should_run_codec_probe(true, tasks_enabled) {
                        if let Err(e) =
                            run_boot_codec_probe(&post_pager, &models, &codec_scratch_dir)
                        {
                            // Same reasoning as `run_post`'s own `Err` arm
                            // below: each model's outcome is journaled by
                            // `run_boot_codec_probe` itself, so reaching
                            // here means the *journal* failed.
                            eprintln!(
                                "bloomery-daemon: codec probe could not record its result: {e}"
                            );
                        } else if let Err(e) =
                            run_boot_g5_probe(&post_pager, &g5_models, &codec_scratch_dir)
                        {
                            // G5 (protocol §1): strictly after G4 completes,
                            // same "the journal itself is the only failure
                            // that reaches here" reasoning as the G4 arm
                            // above — every per-model outcome is journaled
                            // by `run_boot_g5_probe` itself.
                            eprintln!(
                                "bloomery-daemon: G5 refusal probe could not record its result: {e}"
                            );
                        }
                    }
                }
                Err(e) => {
                    // The individual probe outcomes are journaled by
                    // `run_post` itself; reaching here means the *journal*
                    // failed, which is the one thing that cannot be
                    // journaled. The codec probe is skipped too: POST's own
                    // journal write is broken, so there is nowhere honest
                    // left to record the codec probe's outcome either.
                    eprintln!("bloomery-daemon: POST could not record its result: {e}");
                }
            }
        });
    } else {
        // Not silence: a daemon booting with POST off is a daemon whose
        // models will never be profiled, and law 5 refuses every one of them
        // unless the operator also set `allow_unprofiled`.
        let mut p = pager
            .lock()
            .unwrap_or_else(|_| fail("pager poisoned before boot completed"));
        p.journal_degraded("POST disabled by config".to_string())
            .unwrap_or_else(|e| fail(format!("failed to journal degraded boot: {e}")));
        if config.tasks_enabled {
            // The codec probe measures the codec POST would have attached a
            // profile for, and there is no serving window for it to run
            // against either, so every model stays unmeasured for the
            // mutating-verb gate too — one more line beside the one above,
            // because the operator turned the task surface on and deserves
            // to know why it stays refused. `!tasks_enabled` gets no line at
            // all: the surface is dark, and `/status` already tells the
            // truth (`mutating_verbs: false`, `codec_gate: null`).
            p.journal_degraded(POST_DISABLED_CODEC_SKIP_REASON.to_string())
                .unwrap_or_else(|e| fail(format!("failed to journal codec-probe skip: {e}")));
            // G5's own mirrored skip line, gated on "at least one model
            // opted in" — see `G5_POST_DISABLED_SKIP_REASON`'s doc comment.
            if config.models.values().any(|spec| spec.g5_probe()) {
                p.journal_degraded(G5_POST_DISABLED_SKIP_REASON.to_string())
                    .unwrap_or_else(|e| fail(format!("failed to journal G5-probe skip: {e}")));
            }
        }
    }
    // `_handle`'s workers do the actual serving; the main thread just needs
    // to stay alive for the process to keep running. `park()` can wake
    // spuriously, hence the loop.
    loop {
        std::thread::park();
    }
}

/// Without the `llama` feature there is no real substrate to serve
/// inference against, so the daemon refuses to start rather than silently
/// running a `Pager` that can never load a model — a named, loud failure
/// instead of a daemon that looks alive and answers every request with
/// `unknown_model`.
#[cfg(not(feature = "llama"))]
fn run(_config: Config, _journal: Journal) -> ! {
    fail(
        "built without the `llama` feature; this daemon build cannot load models and serves no \
         requests (rebuild with `--features llama`)",
    )
}
