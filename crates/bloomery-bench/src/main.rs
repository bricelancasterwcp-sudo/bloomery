//! `bloomery-bench` — the G2 instrument.
//!
//! ```text
//! bloomery-bench switch --daemon http://127.0.0.1:8181 --model qwen \
//!                       --agents 4 --rounds 13 --window 2048 [--cold]
//! bloomery-bench report --journal <data_dir>/journal/boot-<ts>.jsonl
//! ```
//!
//! `switch` drives the daemon and produces nothing but progress on stdout;
//! `report` reads the journal the daemon wrote and prints the gate document.
//! They are deliberately separate runs over a durable artifact, so the numbers
//! can be recomputed later — and by anyone else — from the same file.

use std::path::PathBuf;
use std::process::ExitCode;

use bloomery_bench::http::Client;
use bloomery_bench::report::{compute_report, report_json};
use bloomery_bench::switch::{self, SwitchOpts};

const USAGE: &str = "usage:
  bloomery-bench switch --daemon <url> --model <name> --agents <n> --rounds <r> \
--window <tokens>
                        --journal <path> [--cold] [--prime-chars <n>] [--max-tokens <n>]
  bloomery-bench report --journal <path>

  --journal is the daemon's journal for this boot (<data_dir>/journal/boot-<ts>.jsonl).
  `switch` reads it to check the run actually produced the switches its pressure
  arithmetic predicted; `report` reads it to compute the gate numbers.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bloomery-bench: {e}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("switch") => run_switch(&args[1..]),
        Some("report") => run_report(&args[1..]),
        Some(other) => Err(format!("unknown subcommand {other:?}\n{USAGE}")),
        None => Err(format!("missing subcommand\n{USAGE}")),
    }
}

fn run_switch(args: &[String]) -> Result<(), String> {
    let opts = SwitchOpts {
        model: required(args, "--model")?.to_string(),
        agents: parse(args, "--agents")?,
        rounds: parse(args, "--rounds")?,
        window: parse(args, "--window")?,
        cold: flag(args, "--cold"),
        prime_chars: optional(args, "--prime-chars")?.unwrap_or(3000),
        max_tokens: optional(args, "--max-tokens")?.unwrap_or(8),
        journal: PathBuf::from(required(args, "--journal")?),
    };
    let client = Client::new(required(args, "--daemon")?)?;
    println!(
        "class={} agents={} rounds={} window={} prime_chars={} max_tokens={}",
        if opts.cold { "cold" } else { "warm" },
        opts.agents,
        opts.rounds,
        opts.window,
        opts.prime_chars,
        opts.max_tokens
    );
    let observed = switch::run(&client, &opts)?;
    println!(
        "done: this run added {} switch samples to {} (warm {} + cold {})",
        observed.total(),
        opts.journal.display(),
        observed.warm,
        observed.cold
    );
    Ok(())
}

fn run_report(args: &[String]) -> Result<(), String> {
    let path = PathBuf::from(required(args, "--journal")?);
    let events = bloomery_core::journal::replay(&path)
        .map_err(|e| format!("replaying {}: {e}", path.display()))?;
    let json = report_json(&compute_report(&events));
    println!(
        "{}",
        serde_json::to_string_pretty(&json).map_err(|e| format!("serializing report: {e}"))?
    );
    Ok(())
}

fn value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn required<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    value(args, name).ok_or_else(|| format!("missing required {name}\n{USAGE}"))
}

fn parse<T: std::str::FromStr>(args: &[String], name: &str) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    let raw = required(args, name)?;
    raw.parse()
        .map_err(|e| format!("{name} {raw:?} is not a valid value: {e}"))
}

fn optional<T: std::str::FromStr>(args: &[String], name: &str) -> Result<Option<T>, String>
where
    T::Err: std::fmt::Display,
{
    match value(args, name) {
        None => Ok(None),
        Some(raw) => raw
            .parse()
            .map(Some)
            .map_err(|e| format!("{name} {raw:?} is not a valid value: {e}")),
    }
}
