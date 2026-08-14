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

/// Builds a real `Pager<SendLlama>` (no models registered — see the module
/// doc) and serves it forever.
#[cfg(feature = "llama")]
fn run(config: Config, journal: Journal) -> ! {
    use std::process::Command;

    use bloomery_core::vram::free_vram_bytes;
    use bloomery_daemon::agents::ImageStore;
    use bloomery_daemon::http::serve;
    use bloomery_daemon::llama_send::SendLlama;
    use bloomery_daemon::pager::Pager;

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
    // it must wire the operator's configured overhead itself.
    pager.set_overhead_bytes(config.overhead_mib.saturating_mul(1024 * 1024));

    let (bound_port, _handle) = serve(pager, config.port);
    println!(
        "bloomery-daemon serving on 127.0.0.1:{bound_port} (data_dir={}; no models registered \
         yet — Task 16 wires config.models)",
        config.data_dir.display()
    );
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
