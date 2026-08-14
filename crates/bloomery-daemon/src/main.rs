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
//! **The `llama` build currently refuses to start too — this is a real,
//! discovered gap, not a stub.** `bloomery_daemon::http::serve` requires
//! `S: Substrate + Send + 'static` (fixed by the Task 14 brief, and needed
//! for real: the worker pool shares `Pager<S>` behind an `Arc<Mutex<_>>`
//! across threads). `bloomery_substrate::llama::LlamaSubstrate` is not
//! `Send` — verified with `cargo check -p bloomery-daemon --features
//! llama`, which fails: `LlamaContext` holds `NonNull<llama_context>` and
//! `*mut llama_sampler`, both raw pointers, so `HashMap<_, LlamaContext<'_>>`
//! (inside `ModelCell`, inside `LlamaSubstrate`) isn't `Send` either. A
//! `Pager<LlamaSubstrate>` genuinely cannot be moved into `serve`'s
//! `Arc<Mutex<_>>` as either type stands today. Fixing this belongs to
//! whoever owns `LlamaSubstrate` (Task 11) or Task 16's boot wiring — most
//! likely a reviewed, justified `unsafe impl Send for LlamaSubstrate` (the
//! daemon's coarse pager `Mutex` already guarantees a context is never
//! touched by two threads at once, which is the property that would make
//! it sound) — not something to bolt on here without that sign-off.

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

/// Refuses to start: `LlamaSubstrate` is not `Send` (see the module doc),
/// so `Pager<LlamaSubstrate>` cannot be moved into `http::serve`'s
/// `Arc<Mutex<_>>` worker pool. `config` and `journal` are otherwise ready
/// to build a real pager the moment that's fixed — this function is where
/// that wiring goes back in.
#[cfg(feature = "llama")]
fn run(_config: Config, _journal: Journal) -> ! {
    fail(
        "built with the `llama` feature, but bloomery_substrate::llama::LlamaSubstrate is not \
         `Send` (LlamaContext holds raw FFI pointers), so it cannot be shared across \
         http::serve's worker pool as Pager<S: Substrate + Send + 'static> requires; this is a \
         known gap for Task 11/16 to close, not yet a servable build",
    )
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
