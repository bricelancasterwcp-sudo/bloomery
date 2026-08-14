//! How many switches this run is *entitled* to, computed from measured facts.
//!
//! # Why this module exists
//!
//! Phase 1's bench refused up front to run the warm class against a daemon
//! that reported a measured VRAM budget. The reason was real: the residency
//! planner charged KV bytes only and never the weights, so on a 16 GB card it
//! answered `Fits` for every agent the bench created, evicted none, and the
//! run finished clean reporting zero warm samples. Refusing was the honest
//! move while that accounting gap existed.
//!
//! Phase 2a closed the gap — `Pager::place` now plans against
//! `budget − Σ loaded weights − Σ resident kv` — so a measured budget is a
//! perfectly legitimate way to run this bench, and in fact the only way to
//! measure switches under *natural* pressure rather than under the
//! unmeasured-probe cap of one resident agent. The blanket refusal is
//! therefore wrong now, and what replaces it is arithmetic: predict how many
//! restores the workload must perform given the budget, the weights and the
//! per-agent KV footprint the daemon itself reports, refuse to run when that
//! prediction is zero, and — the part that cannot be skipped — check the
//! journal afterwards and fail loudly if the run did not deliver.
//!
//! # The prediction
//!
//! `capacity` is how many agent contexts fit alongside the loaded weights:
//! `(budget − overhead − weights) / reserved_per_agent`, or the documented cap
//! of [`UNMEASURED_CAPACITY`] when the probe measured nothing.
//!
//! Every term is read from `/status`, never assumed, and every one of them is
//! a term the pager itself subtracts in `Pager::place`. `reserved_per_agent`
//! is the agent's whole residency reservation — KV cache *plus* the
//! per-context runtime overhead — because that is what placement charges.
//! The 2026-08-14 aborted run is why this is spelled out: the bench and the
//! pager both divided by the bare KV, agreed with each other, and were both
//! wrong by a 304 MiB compute buffer per context.
//!
//! One warm lap opens with the single-use reset agent, which evicts the
//! lowest-priority resident and then suspends, handing that slot back. So a
//! lap starts with the `capacity − 1` highest-priority workers resident, and
//! every worker below them must be restored as its turn comes round:
//!
//! ```text
//! restores_per_lap = agents + 1 − capacity        (warm, floored at 0)
//! restores_per_lap = agents                       (cold: every infer is
//!                                                  preceded by an unload,
//!                                                  which suspends every
//!                                                  resident)
//! ```
//!
//! With `capacity = 1` — the unmeasured-probe regime Phase 1's gate ran in —
//! that is `agents` restores per lap, which is exactly the sample count the
//! G2 run expected. The formula generalises the old expectation rather than
//! replacing it.
//!
//! # Why the floor is one-sided
//!
//! The prediction models the planner's *behaviour*; the numbers it is fed are
//! the daemon's own. A one-context error in `capacity` (rounding, an
//! allocator's opinion about a boundary) moves the prediction by one restore
//! per lap, so the failure threshold sits one restore per lap below it and
//! never below one. Over-delivery is never an error: more switches than
//! predicted means more pressure than predicted, and the samples are still
//! samples. Under-delivery is the failure this instrument exists to make
//! loud — a run that quietly reports `n = 0` is the named bug class, a value
//! that looks like a measurement and is not.

/// bloomery's static VRAM budget as `/status` reports it. `None` on the wire
/// is *unmeasured*, never zero, and it is carried as its own variant here so
/// no arithmetic can accidentally treat "we did not measure" as "there is no
/// memory".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Budget {
    Measured(u64),
    Unmeasured,
}

/// The residency cap the pager documents for an unmeasured budget: one
/// resident agent, journaled as a degradation. Mirrored here because the
/// prediction has to know what regime it is predicting for.
pub const UNMEASURED_CAPACITY: u64 = 1;

/// The pressure arithmetic for one run, and the sample counts it licenses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pressure {
    pub budget: Budget,
    /// The daemon-level margin held back from the placement budget.
    pub overhead_bytes: u64,
    pub weights_bytes: u64,
    /// What one agent reserves when resident: KV plus per-context overhead,
    /// as `/status` reports it per agent.
    pub reserved_bytes_per_agent: u64,
    pub agents: usize,
    pub rounds: usize,
    pub cold: bool,
    /// Contexts that fit alongside the weights.
    pub capacity_contexts: u64,
    /// Restores one lap should need.
    pub predicted_per_lap: u64,
    /// Restores one lap must deliver, or the run failed.
    pub floor_per_lap: u64,
}

impl Pressure {
    /// Builds the arithmetic. A reserved figure of zero is an instrument
    /// failure, not a divide-by-zero: `/status` reporting no KV footprint for
    /// an agent it just created means the numbers this check rests on are not
    /// the numbers it thinks they are.
    pub fn compute(
        budget: Budget,
        overhead_bytes: u64,
        weights_bytes: u64,
        reserved_bytes_per_agent: u64,
        agents: usize,
        rounds: usize,
        cold: bool,
    ) -> Result<Pressure, String> {
        if reserved_bytes_per_agent == 0 {
            return Err(
                "/status reports kv_bytes = 0 for the bench's agents; the pressure arithmetic \
                 has nothing to divide by and the run's sample count could not be checked"
                    .to_string(),
            );
        }
        let capacity_contexts = match budget {
            Budget::Unmeasured => UNMEASURED_CAPACITY,
            Budget::Measured(bytes) => {
                bytes
                    .saturating_sub(overhead_bytes)
                    .saturating_sub(weights_bytes)
                    / reserved_bytes_per_agent
            }
        };
        let predicted_per_lap = if cold {
            agents as u64
        } else {
            (agents as u64 + 1).saturating_sub(capacity_contexts)
        };
        let floor_per_lap = if cold {
            agents as u64
        } else {
            predicted_per_lap.saturating_sub(1).max(1)
        };
        Ok(Pressure {
            budget,
            overhead_bytes,
            weights_bytes,
            reserved_bytes_per_agent,
            agents,
            rounds,
            cold,
            capacity_contexts,
            predicted_per_lap,
            floor_per_lap,
        })
    }

    /// Samples the whole run should produce.
    pub fn predicted(&self) -> usize {
        self.rounds.saturating_mul(self.predicted_per_lap as usize)
    }

    /// Samples the whole run must produce, or [`check`] fails it.
    pub fn floor(&self) -> usize {
        self.rounds.saturating_mul(self.floor_per_lap as usize)
    }

    /// False when the arithmetic says nothing will ever be evicted — every
    /// context fits alongside the weights with room to spare, so the laps
    /// would run to completion and measure nothing. That is the one
    /// precondition worth refusing *before* spending the run.
    pub fn has_pressure(&self) -> bool {
        self.predicted_per_lap > 0
    }

    /// The arithmetic as an operator reads it. Printed on the way in and
    /// quoted again in any failure, so the evidence and the error carry the
    /// same numbers.
    pub fn arithmetic(&self) -> String {
        let budget = match self.budget {
            Budget::Measured(bytes) => format!("{bytes} bytes ({})", mib(bytes)),
            Budget::Unmeasured => {
                format!("unmeasured (residency capped at {UNMEASURED_CAPACITY} resident agent)")
            }
        };
        format!(
            "  budget            {budget}\n  \
             daemon overhead   {} bytes ({})\n  \
             loaded weights    {} bytes ({})\n  \
             reserved / agent  {} bytes ({}) — kv + per-context overhead\n  \
             capacity          {} contexts alongside the weights\n  \
             class             {}\n  \
             agents/rounds     {} / {}\n  \
             predicted         {} restores per lap, {} for the run\n  \
             floor             {} restores per lap, {} for the run (a floor, \
             not an expectation: over-delivery is not an error)",
            self.overhead_bytes,
            mib(self.overhead_bytes),
            self.weights_bytes,
            mib(self.weights_bytes),
            self.reserved_bytes_per_agent,
            mib(self.reserved_bytes_per_agent),
            self.capacity_contexts,
            if self.cold { "cold" } else { "warm" },
            self.agents,
            self.rounds,
            self.predicted_per_lap,
            self.predicted(),
            self.floor_per_lap,
            self.floor(),
        )
    }
}

/// What the journal actually recorded for this run — the counts *after* the
/// laps minus the counts before them, so a second bench run against the same
/// daemon boot cannot inherit the first one's samples and hide a shortfall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Observed {
    pub warm: usize,
    pub cold: usize,
}

impl Observed {
    pub fn total(&self) -> usize {
        self.warm.saturating_add(self.cold)
    }

    /// The count for the class this run asked for.
    pub fn requested(&self, cold: bool) -> usize {
        if cold {
            self.cold
        } else {
            self.warm
        }
    }
}

/// The exit check: did the run produce the switches its own arithmetic said
/// it would?
///
/// Two ways to fail, and both are the same bug wearing different clothes:
///
/// * fewer samples than the floor — the pressure the run was built on did not
///   materialise, so whatever `report` prints is computed over a sample set
///   nobody arranged;
/// * zero samples *in the requested class* — a warm run whose images all
///   spilled to NVMe measured something real, but not the thing it claimed to
///   be measuring, and the operator has to be told rather than left to read a
///   `null` percentile as a pass.
///
/// `status` is `/status` as it read at the end of the run, quoted verbatim
/// into the failure so the operator sees the budget and
/// `loaded_weights_bytes` next to the count they explain.
pub fn check(p: &Pressure, observed: &Observed, status: &str) -> Result<(), String> {
    let class = if p.cold { "cold" } else { "warm" };
    let shortfall = observed.total() < p.floor();
    let wrong_class = observed.requested(p.cold) == 0;
    if !shortfall && !wrong_class {
        return Ok(());
    }
    let verdict = if shortfall {
        format!(
            "produced {} switch samples, below the floor of {}",
            observed.total(),
            p.floor()
        )
    } else {
        format!(
            "produced {} switch samples but none of them landed in the {class} class it was run \
             for",
            observed.total()
        )
    };
    Err(format!(
        "the {class} run {verdict}.\n\
         observed (this run, from the journal): warm {} + cold {} = {}\n\
         pressure arithmetic:\n{}\n\
         /status at end of run: {status}\n\
         A run that reports fewer switches than it was built to force has not measured a fast \
         daemon — it has measured a workload that never switched. Do not read percentiles off \
         it.",
        observed.warm,
        observed.cold,
        observed.total(),
        p.arithmetic(),
    ))
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    /// 16384 tokens at 57344 bytes per token — the live 2a run's agent.
    const KV: u64 = 939_524_096;
    /// KV plus the 384 MiB per-context reservation the daemon wires by
    /// default: what one of those agents actually costs residency.
    const RESERVED: u64 = KV + 384 * MIB;

    fn measured(agents: usize, rounds: usize, cold: bool) -> Pressure {
        Pressure::compute(
            Budget::Measured(14 * GIB),
            1024 * MIB,
            8 * GIB + GIB / 10,
            RESERVED,
            agents,
            rounds,
            cold,
        )
        .expect("nonzero reservation")
    }

    #[test]
    fn measured_budget_capacity_is_budget_minus_weights_over_kv() {
        let p = measured(8, 8, false);
        // (14 − 1 − 8.1) GiB = 4.9 GiB over a 1.25 GiB reservation -> 3.
        assert_eq!(p.capacity_contexts, 3);
    }

    #[test]
    fn warm_lap_restores_every_worker_the_capacity_cannot_hold() {
        let p = measured(8, 8, false);
        assert_eq!(p.predicted_per_lap, 6);
        assert_eq!(p.predicted(), 48);
    }

    #[test]
    fn warm_floor_sits_one_restore_per_lap_below_the_prediction() {
        let p = measured(8, 8, false);
        assert_eq!(p.floor_per_lap, 5);
        assert_eq!(p.floor(), 40);
    }

    #[test]
    fn unmeasured_budget_predicts_the_phase_one_expectation() {
        let p = Pressure::compute(Budget::Unmeasured, 0, 8 * GIB, RESERVED, 8, 7, false)
            .expect("nonzero reservation");
        assert_eq!(p.capacity_contexts, UNMEASURED_CAPACITY);
        assert_eq!(p.predicted(), 56, "one restore per worker per lap");
        assert_eq!(p.floor(), 49);
    }

    #[test]
    fn cold_class_restores_every_worker_every_lap_whatever_fits() {
        let p = measured(8, 8, true);
        assert_eq!(p.predicted_per_lap, 8);
        assert_eq!(p.floor_per_lap, 8, "the unload makes this exact, not slack");
    }

    #[test]
    fn a_budget_that_holds_every_context_has_no_pressure() {
        // 9 contexts fit and only 8 agents plus the reset agent exist.
        let p = Pressure::compute(Budget::Measured(9 * RESERVED), 0, 0, RESERVED, 8, 8, false)
            .expect("nonzero reservation");
        assert_eq!(p.capacity_contexts, 9);
        assert_eq!(p.predicted_per_lap, 0);
        assert!(!p.has_pressure());
    }

    #[test]
    fn a_budget_one_context_short_still_has_pressure() {
        let p = Pressure::compute(Budget::Measured(8 * RESERVED), 0, 0, RESERVED, 8, 8, false)
            .expect("nonzero reservation");
        assert_eq!(p.predicted_per_lap, 1);
        assert!(p.has_pressure());
    }

    #[test]
    fn zero_kv_is_an_instrument_failure_not_a_division() {
        let err = Pressure::compute(Budget::Measured(14 * GIB), 0, 0, 0, 8, 8, false)
            .expect_err("kv_bytes = 0 must be refused");
        assert!(err.contains("kv_bytes = 0"), "{err}");
    }

    #[test]
    fn a_run_that_meets_its_floor_passes() {
        let p = measured(8, 8, false);
        let observed = Observed { warm: 40, cold: 0 };
        assert_eq!(check(&p, &observed, "{}"), Ok(()));
    }

    #[test]
    fn a_run_below_its_floor_fails_with_the_whole_arithmetic() {
        let p = measured(8, 8, false);
        let observed = Observed { warm: 39, cold: 0 };
        let err = check(&p, &observed, "{\"loaded_weights_bytes\":8697308774}")
            .expect_err("39 samples is below the floor of 40");
        assert!(err.contains("below the floor of 40"), "{err}");
        assert!(err.contains("warm 39 + cold 0 = 39"), "{err}");
        assert!(err.contains("loaded_weights_bytes"), "{err}");
        assert!(err.contains("capacity          3 contexts"), "{err}");
    }

    #[test]
    fn a_silent_zero_sample_run_fails() {
        let p = measured(8, 8, false);
        let err = check(&p, &Observed::default(), "{}").expect_err("n = 0 must never pass");
        assert!(err.contains("below the floor"), "{err}");
    }

    #[test]
    fn a_warm_run_that_produced_only_cold_samples_fails() {
        let p = measured(8, 8, false);
        // Above the floor in total, but nothing in the class it ran for.
        let observed = Observed { warm: 0, cold: 48 };
        let err = check(&p, &observed, "{}").expect_err("wrong class must not pass");
        assert!(
            err.contains("none of them landed in the warm class"),
            "{err}"
        );
    }

    #[test]
    fn cold_samples_count_toward_a_warm_run_s_floor() {
        // A warm run whose images spilled to NVMe still switched; the floor is
        // about pressure, and the class check below is what catches the mix.
        let p = measured(8, 8, false);
        let observed = Observed { warm: 1, cold: 39 };
        assert_eq!(check(&p, &observed, "{}"), Ok(()));
    }
}
