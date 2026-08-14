//! Daemon entry point.
//!
//! For now (Task 12) this only proves the boot path: parse `--config`, load
//! it, open a fresh boot journal, and record `Event::Boot`. The actual
//! request-serving loop lands in Task 14.

use bloomery_core::journal::{Event, Journal};
use bloomery_daemon::config::load_config;
use std::path::PathBuf;

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

    println!(
        "bloomery-daemon listening on port {} (data_dir={})",
        config.port,
        config.data_dir.display()
    );
    std::process::exit(0);
}
