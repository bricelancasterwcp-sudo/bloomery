//! The `switch` driver: make the daemon page agents in and out, on purpose,
//! enough times for the gate to have a sample.
//!
//! # Why the protocol looks like this
//!
//! Two properties of the daemon shape every line below, and both were read
//! out of the code rather than assumed:
//!
//! 1. **The residency planner only evicts *strictly lower priority* idle
//!    residents** (`bloomery_core::scheduler::plan_residency`). A naive
//!    round-robin over N same-priority agents therefore does not switch at
//!    planning time; since Phase 2a an equal-priority refusal that has waited
//!    out the time-sharing quantum is retried as an eviction of the LRU peer,
//!    but a bench that leaned on that would be measuring the quantum's clock,
//!    not a switch. The workers here carry strictly ascending priorities and
//!    are always visited in that order, so every step is an ordinary planner
//!    eviction and the time-sharing path never fires.
//!
//! 2. **A RAM-tier KV image can only be produced by an eviction.**
//!    `Pager::suspend` always spills to NVMe, so it can never set up a warm
//!    switch — it can only end one. That is why a lap cannot simply wrap
//!    around: after the top-priority worker is resident, nothing below it may
//!    evict it. Each warm lap therefore opens with a **reset agent** — a
//!    fresh, top-priority, single-use agent that evicts the incumbent worker
//!    (giving it a RAM image), then suspends itself to hand the VRAM back.
//!    The reset agent is created fresh every lap precisely so it never has an
//!    image of its own to restore, and so contributes no sample.
//!
//! # What one lap produces
//!
//! * `--warm` (default): exactly `agents` warm samples per lap — each worker
//!   evicts its predecessor and restores its own RAM image, with the weights
//!   resident throughout.
//! * `--cold`: exactly `agents` cold samples per lap — the model is unloaded
//!   before every inference, so each switch pays a full weight reload and
//!   reads its image back off NVMe.
//!
//! The bench measures nothing itself; it only issues requests. Every number
//! the gate is read from was recorded by the pager, in the daemon, around the
//! operation it names, and lives in the journal.
//!
//! # The sample check
//!
//! A run that switched nothing still exits zero and still leaves a journal
//! `report` will happily read — as `n: 0` with `null` percentiles, which is
//! honest but easy to skim past. So the driver reads the daemon's journal
//! itself, before and after the laps, and refuses to end successfully unless
//! the run delivered the switches its own pressure arithmetic said it would.
//! See [`crate::pressure`] for that arithmetic and for the Phase 1 preflight
//! rule this replaced.

use std::path::{Path, PathBuf};

use crate::http::Client;
use crate::pressure::{Budget, Observed, Pressure};
use crate::report::compute_report;

/// Priority of the single-use reset agent. Above every worker so it can
/// always evict the incumbent; below `u8::MAX` for no reason but taste.
const RESET_PRIORITY: u8 = 250;
/// Worker priorities are `PRIORITY_STEP * (i + 1)`, which must stay strictly
/// below [`RESET_PRIORITY`].
const PRIORITY_STEP: u8 = 10;
/// The reset agent only needs a non-empty KV cache so that suspending it has
/// something to save — `save_state` refuses a zero-length state.
const RESET_MAX_TOKENS: u32 = 1;

pub struct SwitchOpts {
    pub model: String,
    pub agents: usize,
    pub rounds: usize,
    pub window: u32,
    pub cold: bool,
    pub prime_chars: usize,
    pub max_tokens: u32,
    /// The daemon's journal for this boot. Required: it is the only place the
    /// driver can see whether the workload actually switched anything, and a
    /// run that cannot be checked is a run that can report `n = 0` in silence.
    pub journal: PathBuf,
}

struct Agent {
    id: String,
    window_tokens: u32,
}

/// Runs the whole protocol and returns the switch samples *this run* added to
/// the journal, split by class.
///
/// The count is the run's own delta, not the journal's total: a second bench
/// run against the same daemon boot would otherwise inherit the first one's
/// samples and could pass the check on them alone.
pub fn run(client: &Client, opts: &SwitchOpts) -> Result<Observed, String> {
    validate(opts)?;
    preflight(client, opts)?;

    let workers = create_workers(client, opts)?;
    let prime = prime_prompt(opts.prime_chars);
    prime_workers(client, opts, &workers, &prime)?;

    // Measured only now: the weights are loaded (the priming inferences
    // loaded them) and every worker's KV footprint is a number the daemon has
    // committed to, so the arithmetic runs on facts rather than on the
    // operator's expectations about the card.
    let pressure = measure_pressure(client, opts, &workers)?;
    println!("pressure:\n{}", pressure.arithmetic());
    if !pressure.has_pressure() {
        return Err(format!(
            "no residency pressure: {} contexts fit alongside the weights, which is more than \
             the {} workers plus the reset agent this run creates. The planner will place every \
             agent and evict none, and the laps would finish clean having measured nothing. \
             Raise --window or --agents, or give the daemon a smaller budget.\n{}",
            pressure.capacity_contexts,
            opts.agents,
            pressure.arithmetic()
        ));
    }
    let baseline = count_samples(&opts.journal)?;

    for lap in 0..opts.rounds {
        if !opts.cold {
            reset_residency(client, opts, lap)?;
        }
        for worker in &workers {
            if opts.cold {
                client.expect("POST", &format!("/models/{}/unload", opts.model), "", 204)?;
            }
            let reply = infer(client, &worker.id, &round_prompt(lap), opts.max_tokens)?;
            println!(
                "lap {}/{} {} infer ok ({} prompt + {} completion tokens, {} ms)",
                lap + 1,
                opts.rounds,
                worker.id,
                reply.0,
                reply.1,
                reply.2
            );
        }
    }

    let after = count_samples(&opts.journal)?;
    let observed = Observed {
        warm: after.warm.saturating_sub(baseline.warm),
        cold: after.cold.saturating_sub(baseline.cold),
    };
    crate::pressure::check(&pressure, &observed, &status_line(client)?)?;
    Ok(observed)
}

/// Reads the pressure arithmetic off the daemon: its static budget, the
/// weights it currently holds, and the KV footprint it assigned this run's
/// agents.
///
/// The per-agent footprint is the largest one among the workers, not the mean:
/// the capacity that matters is how many of the *biggest* contexts fit, and
/// the window law can hand different agents different windows if the binding
/// term moves under them. `/status`'s per-agent `kv_bytes` carries the whole
/// residency reservation (KV plus the per-context runtime overhead), which is
/// exactly what placement charges.
fn measure_pressure(
    client: &Client,
    opts: &SwitchOpts,
    workers: &[Agent],
) -> Result<Pressure, String> {
    let status = client.expect("GET", "/status", "", 200)?.json()?;
    let weights_bytes = status["loaded_weights_bytes"].as_u64().ok_or_else(|| {
        format!(
            "/status has no loaded_weights_bytes; this daemon predates the reservation-budget \
             accounting the pressure arithmetic rests on: {status}"
        )
    })?;
    // Read, never assumed: the bench must divide by the same numbers
    // `Pager::place` subtracts. The 2026-08-14 aborted run is the reason —
    // a bench carrying its own copy of the per-context cost would have
    // agreed with a pager that was wrong.
    let overhead_bytes = status["overhead_bytes"].as_u64().ok_or_else(|| {
        format!(
            "/status has no overhead_bytes; this daemon predates the per-context reservation \
             accounting the pressure arithmetic rests on: {status}"
        )
    })?;
    let reserved_bytes_per_agent = status["agents"]
        .as_array()
        .map(|agents| {
            agents
                .iter()
                .filter(|a| {
                    a["id"]
                        .as_str()
                        .is_some_and(|id| workers.iter().any(|w| w.id == id))
                })
                .filter_map(|a| a["kv_bytes"].as_u64())
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    Pressure::compute(
        budget(&status),
        overhead_bytes,
        weights_bytes,
        reserved_bytes_per_agent,
        opts.agents,
        opts.rounds,
        opts.cold,
    )
}

/// `/status`'s `free_vram_bytes`: `null` there is unmeasured, never zero.
fn budget(status: &serde_json::Value) -> Budget {
    match status["free_vram_bytes"].as_u64() {
        Some(bytes) => Budget::Measured(bytes),
        None => Budget::Unmeasured,
    }
}

/// The three supply-side numbers of the reservation budget, as one line for
/// the operator and for any failure that has to explain a sample count.
fn status_line(client: &Client) -> Result<String, String> {
    let status = client.expect("GET", "/status", "", 200)?.json()?;
    Ok(format!(
        "free_vram_bytes={} overhead_bytes={} ctx_overhead_bytes={} loaded_weights_bytes={} \
         resident_kv_bytes={}",
        status["free_vram_bytes"],
        status["overhead_bytes"],
        status["ctx_overhead_bytes"],
        status["loaded_weights_bytes"],
        status["resident_kv_bytes"]
    ))
}

/// Counts the switch samples in the daemon's journal with the same arithmetic
/// `report` uses — the check and the published number must never be able to
/// disagree about what a sample is.
fn count_samples(path: &Path) -> Result<Observed, String> {
    let events = bloomery_core::journal::replay(path)
        .map_err(|e| format!("reading the daemon's journal at {}: {e}", path.display()))?;
    let report = compute_report(&events);
    Ok(Observed {
        warm: report.warm.n,
        cold: report.cold.n,
    })
}

fn validate(opts: &SwitchOpts) -> Result<(), String> {
    if opts.agents < 2 {
        return Err(
            "--agents must be at least 2: a switch needs something to switch away from".to_string(),
        );
    }
    let top = u64::from(PRIORITY_STEP) * (opts.agents as u64);
    if top >= u64::from(RESET_PRIORITY) {
        return Err(format!(
            "--agents {} needs worker priorities up to {top}, which collides with the reset \
             agent's {RESET_PRIORITY}",
            opts.agents
        ));
    }
    if opts.rounds == 0 {
        return Err("--rounds must be at least 1".to_string());
    }
    Ok(())
}

/// Checks the daemon is serving what we think it is, and that the journal the
/// exit check will read is the one this daemon is writing.
///
/// It no longer has an opinion about *how* residency pressure is arranged.
/// Phase 1's preflight refused the warm class outright against a measured VRAM
/// budget, because the planner charged KV bytes only and would have placed
/// every agent and evicted none; Phase 2a charges the weights too, so a
/// measured budget is now the natural way to run this bench. The pressure
/// question moved to where it can be answered with measured numbers instead of
/// a proxy — [`measure_pressure`], after the workers exist and the weights are
/// loaded — and to the exit check that will not let a run without pressure
/// report success.
fn preflight(client: &Client, opts: &SwitchOpts) -> Result<(), String> {
    let status = client.expect("GET", "/status", "", 200)?.json()?;

    let known = status["models"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .any(|m| m["name"].as_str() == Some(opts.model.as_str()))
        })
        .unwrap_or(false);
    if !known {
        return Err(format!(
            "daemon at {} does not serve model {:?}; /status says {}",
            client.addr(),
            opts.model,
            status["models"]
        ));
    }

    println!(
        "preflight: daemon {} tier={} free_vram_bytes={} loaded_weights_bytes={}",
        client.addr(),
        status["tier"],
        status["free_vram_bytes"],
        status["loaded_weights_bytes"]
    );

    // Read now rather than at the end: an unreadable journal must cost the
    // operator a second, not the whole run.
    count_samples(&opts.journal)?;
    Ok(())
}

fn create_workers(client: &Client, opts: &SwitchOpts) -> Result<Vec<Agent>, String> {
    let mut workers = Vec::with_capacity(opts.agents);
    for i in 0..opts.agents {
        let priority = PRIORITY_STEP.saturating_mul((i + 1) as u8);
        let agent = create_agent(client, &opts.model, priority, opts.window)?;
        println!(
            "created {} priority={priority} window_tokens={}",
            agent.id, agent.window_tokens
        );
        if agent.window_tokens != opts.window {
            println!(
                "  note: asked for {} tokens, got {} — the window law bound it elsewhere",
                opts.window, agent.window_tokens
            );
        }
        workers.push(agent);
    }
    Ok(workers)
}

/// Gives every worker a real conversation to carry, so a switch moves a KV
/// image worth moving rather than a handful of cells.
///
/// Cold mode suspends each worker immediately after priming it, which spills
/// its image to NVMe: the cold class is defined as *image on NVMe*, and
/// without this the first lap would restore RAM-tier images and still be
/// counted cold (correctly — the weights reloaded) while being cheaper than
/// the class it represents.
fn prime_workers(
    client: &Client,
    opts: &SwitchOpts,
    workers: &[Agent],
    prime: &str,
) -> Result<(), String> {
    for (i, worker) in workers.iter().enumerate() {
        let (prompt_tokens, completion_tokens, duration_ms) =
            infer(client, &worker.id, prime, opts.max_tokens)?;
        println!(
            "primed {} ({prompt_tokens} prompt + {completion_tokens} completion tokens, \
             {duration_ms} ms)",
            worker.id
        );
        if i == 0 {
            check_headroom(opts, worker, prompt_tokens)?;
        }
        if opts.cold {
            client.expect("POST", &format!("/agents/{}/suspend", worker.id), "", 204)?;
        }
    }
    Ok(())
}

/// Refuses to start the laps if they would run the agents out of context.
///
/// `infer` continues an agent's KV cache rather than resetting it, so every
/// lap adds tokens permanently. Overflowing the window mid-run does not
/// corrupt anything — the substrate refuses the request — but it ends the run
/// with a partial sample count, which is exactly the kind of "a number that
/// means something other than it says" this instrument must not produce.
fn check_headroom(opts: &SwitchOpts, worker: &Agent, prime_tokens: u32) -> Result<(), String> {
    // Worst-case tokenization of a short ASCII prompt: one token per two
    // characters. Deliberately pessimistic — the guard exists to be wrong in
    // the safe direction.
    let per_lap = (round_prompt(0).len() as u64 / 2 + 1) + u64::from(opts.max_tokens);
    let needed =
        u64::from(prime_tokens) + u64::from(opts.max_tokens) + (opts.rounds as u64) * per_lap;
    if needed > u64::from(worker.window_tokens) {
        return Err(format!(
            "not enough context to finish: priming used {prime_tokens} tokens and {} laps need \
             ~{} more, for {needed} of a {} token window. Lower --prime-chars or --rounds.",
            opts.rounds,
            (opts.rounds as u64) * per_lap,
            worker.window_tokens
        ));
    }
    Ok(())
}

/// One lap's reset: a fresh top-priority agent evicts the incumbent worker
/// (which is what parks that worker's image in RAM), takes a one-token turn so
/// it has a state worth saving, and suspends itself to hand the VRAM back.
///
/// Fresh every lap on purpose: an agent that has been suspended once owns an
/// NVMe image, and resuming *that* would be a cold switch dressed as
/// bookkeeping. A brand-new agent has no image, so its own page-in produces no
/// `ResumeLoad` and therefore no sample.
fn reset_residency(client: &Client, opts: &SwitchOpts, lap: usize) -> Result<(), String> {
    let reset = create_agent(client, &opts.model, RESET_PRIORITY, opts.window)?;
    infer(
        client,
        &reset.id,
        &format!("reset {lap}\n"),
        RESET_MAX_TOKENS,
    )?;
    client.expect("POST", &format!("/agents/{}/suspend", reset.id), "", 204)?;
    Ok(())
}

fn create_agent(client: &Client, model: &str, priority: u8, window: u32) -> Result<Agent, String> {
    let body = serde_json::json!({
        "model": model,
        "priority": priority,
        "window_cap": window,
    })
    .to_string();
    let value = client.expect("POST", "/agents", &body, 201)?.json()?;
    let id = value["id"]
        .as_str()
        .ok_or_else(|| format!("POST /agents returned no id: {value}"))?
        .to_string();
    let window_tokens = value["window_tokens"]
        .as_u64()
        .ok_or_else(|| format!("POST /agents returned no window_tokens: {value}"))?;
    Ok(Agent {
        id,
        window_tokens: u32::try_from(window_tokens).unwrap_or(u32::MAX),
    })
}

/// One inference. Returns `(prompt_tokens, completion_tokens, duration_ms)` —
/// the daemon's own counts, used for progress and for the headroom guard, and
/// never for the gate.
fn infer(
    client: &Client,
    id: &str,
    prompt: &str,
    max_tokens: u32,
) -> Result<(u32, u32, u64), String> {
    let body = serde_json::json!({"prompt": prompt, "max_tokens": max_tokens}).to_string();
    let value = client
        .expect("POST", &format!("/agents/{id}/infer"), &body, 200)?
        .json()?;
    let field = |name: &str| -> Result<u64, String> {
        value[name]
            .as_u64()
            .ok_or_else(|| format!("infer reply for {id} has no {name}: {value}"))
    };
    Ok((
        u32::try_from(field("prompt_tokens")?).unwrap_or(u32::MAX),
        u32::try_from(field("completion_tokens")?).unwrap_or(u32::MAX),
        field("duration_ms")?,
    ))
}

/// Deterministic filler, built to tokenize like ordinary prose (~4 chars per
/// token) so `--prime-chars` is a predictable lever on how big the KV image
/// that gets paged around actually is.
fn prime_prompt(chars: usize) -> String {
    const WORDS: &str = "the pager decides what lives in video memory and what waits on disk \
                         while an agent keeps its conversation intact across every move it is \
                         asked to make between one turn and the next one after that ";
    let mut prompt = String::with_capacity(chars + WORDS.len());
    while prompt.len() < chars {
        prompt.push_str(WORDS);
    }
    prompt.truncate(chars);
    prompt
}

fn round_prompt(lap: usize) -> String {
    format!("\nstep {lap}: continue.\n")
}
