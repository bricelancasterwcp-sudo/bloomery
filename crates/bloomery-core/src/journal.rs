use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

pub type AgentId = String;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "event")]
pub enum Event {
    Boot {
        version: String,
    },
    Post {
        model: String,
        outcome: String,
        profile_path: Option<String>,
    },
    Degraded {
        reason: String,
    },
    AgentCreated {
        id: AgentId,
        model: String,
        priority: u8,
        window_tokens: u32,
        bound_by: String,
        budget_granted: u64,
    },
    SchedulerDecision {
        id: AgentId,
        decision: String,
        evicted: Vec<AgentId>,
    },
    Refusal {
        id: AgentId,
        needed_tokens: u64,
        window_tokens: u32,
        detail: String,
    },
    BudgetRefused {
        id: AgentId,
        remaining: u64,
        requested: u64,
    },
    InferStarted {
        id: AgentId,
        prompt: String,
        prompt_sha256: String,
    },
    InferCompleted {
        id: AgentId,
        prompt_tokens: u32,
        completion_tokens: u32,
        duration_ms: u64,
    },
    ContractViolation {
        id: AgentId,
        kind: String,
    },
    PagerOp {
        id: AgentId,
        op: PagerOpKind,
        bytes: u64,
        duration_ms: u64,
        image_tier: String,
    },
    ModelLoaded {
        model: String,
        duration_ms: u64,
    },
    ModelUnloaded {
        model: String,
    },
    /// An agent left the table for good (not suspended — removed, e.g. the
    /// `/v1` shim's ephemeral-agent cleanup). `reason` is a free-text
    /// operator-facing note, not a machine-matched code.
    AgentRemoved {
        id: AgentId,
        reason: String,
    },
    /// One step of a running task. Emitted by the 2b task loop; defined now
    /// so the schema version is stable — 2a never appends this variant.
    TaskStep {
        id: AgentId,
        step: u32,
        verb: String,
        outcome: String,
        duration_ms: u64,
        /// The action's model-supplied arguments, verbatim and in order
        /// (turn-5 spec §3): read -> [path] (+ "lines=a-b"); find ->
        /// [pattern, path]; patch -> [path] (never the body); run -> argv;
        /// done / unparseable -> []. `#[serde(default)]` so pre-turn-5 rows
        /// replay with an empty list.
        #[serde(default)]
        args: Vec<String>,
        /// The window-ladder rung (1-4) this step's prompt was ACTUALLY
        /// sent at — for a parse-failure row, the rung its own failed
        /// attempt used (window-ladder design doc §6,
        /// `docs/superpowers/specs/2026-08-27-window-ladder-design.md`).
        /// A named default, not a bare `#[serde(default)]`: a row carrying
        /// no `rung` key must replay as 1 — what every pre-ladder row WAS,
        /// there being no ladder to climb — never the nonexistent rung 0
        /// (the `default_expect_patch` compat pattern).
        #[serde(default = "default_rung_one")]
        rung: u32,
    },
    /// One codec-probe fixture run (G4/G5 instrument). `detail` is the last
    /// patch step's outcome, or the terminal status when no patch step ran
    /// (patch-class); the failing leg's name, or the clean-refusal message,
    /// for a refuse-class fixture (G5 protocol §2).
    CodecFixture {
        model: String,
        fixture_set: String,
        fixture: String,
        codec: String, // "search_replace" | "whole_file"
        landed: bool,
        steps: u32,
        detail: String,
        /// The fixture's class (`"patch"` | `"refuse"`, G5 design doc §2).
        /// `#[serde(default)]` so every `CodecFixture` row journaled before
        /// this field existed keeps replaying: an old row carries no
        /// `expect` key at all, and the absent-key default is `"patch"` —
        /// exactly what every fixture WAS before G5, so old journals replay
        /// byte-identically (the compat pin, `journal_test.rs`).
        #[serde(default = "default_expect_patch")]
        expect: String,
        /// The agent that ran this fixture — the exact join key to its
        /// `TaskStep` rows (`CodecFixture.agent == TaskStep.id`), replacing
        /// the ordinal join (turn-5 spec §3). `None` on pre-turn-5 rows.
        #[serde(default)]
        agent: Option<AgentId>,
    },
    /// The per-model G4 verdict, emitted exactly once per completed probe
    /// (never for an aborted one — unmeasured is not an event, it is the
    /// absence of this event plus a Degraded reason).
    CodecVerdict {
        model: String,
        fixture_set: String,
        codec: String,
        landed: u32,
        n: u32,
        interval95: [f64; 2],
        provisional: bool,
        mutating_verbs: bool,
        detail: String, // names the lens: "applies_and_parses under bloomery-task-envelope-v1" (+ codec-selection provenance)
    },
    /// A mixed-set (G5) verdict: per-class results, never blended (G5
    /// protocol §3: "Classes are never blended"). Advisory — emitted
    /// alongside, never instead of, G4's `CodecVerdict` machinery: a set
    /// with any refuse fixture emits this event and skips `CodecVerdict`
    /// entirely, never both for the same probe.
    CodecVerdictMixed {
        model: String,
        fixture_set: String,
        codec: String,
        envelope: String,
        patch_landed: u32,
        patch_n: u32,
        patch_interval95: [f64; 2],
        patch_provisional: bool,
        refuse_landed: u32,
        refuse_n: u32,
        refuse_interval95: [f64; 2],
        refuse_provisional: bool,
        /// Both class decisions cleared their ≥80% floor (G5 protocol §3) —
        /// the done-trust mark `/status` surfaces. Never itself a decision:
        /// it is the AND of the two independent, already-decided class
        /// gates.
        done_trust: bool,
        detail: String, // codec-selection provenance only; the lens lives in `envelope` above
    },
    /// A model's current profile was named the drift-cumulative baseline
    /// (drift-watch design §2). Emitted once per blessing, never inferred:
    /// the provenance of every baseline is explicit, so a replay can always
    /// say *who* decided this document is the reference.
    ///
    /// `provenance` is a **family, not a closed set**, and a consumer reads it
    /// by prefix:
    ///
    /// - exactly `"auto-first-profile"` — the first successful POST for a
    ///   model, which blesses itself so the cumulative comparison has a
    ///   reference at all. This daemon never auto-blesses over an existing
    ///   baseline, so this spelling never carries a parenthetical.
    /// - prefix `"operator"` — an explicit operator action. Bare `"operator"`
    ///   when the model had no baseline; `"operator (replaced <sha256>)"` when
    ///   this blessing overwrote one, where `<sha256>` is the superseded
    ///   document's full digest — or, when those bytes existed but could not be
    ///   read, a sentence saying so in place of the digest.
    ///
    /// **Re-blessing carries the superseded identity IN THIS ROW**, in the
    /// parenthetical above. Rows are append-only and the old row is never
    /// edited, but the old row cannot tell a replay that it *stopped* being the
    /// baseline — only the replacing row knows that, and the digest it names is
    /// what ties the two together (it equals the earlier row's `sha`). The
    /// replaced document's bytes are gone the instant the blessing lands, so
    /// this digest is all that survives of it.
    ///
    /// `sha` is the sha256 of the blessed document's **bytes**, not of its
    /// path: the row's path claim is checkable with `sha256sum` against the
    /// file it names (design §5).
    Blessed {
        model: String,
        profile_path: String,
        sha: String,
        provenance: String,
    },
    /// An admission block set by the drift watch, or cleared by an
    /// operator (verdict-gated-admission design §4). Two rows at most per
    /// model per boot: one when a confirmed cumulative regression set it,
    /// one if the operator cleared it. A replay reconstructs which models
    /// were held out, by which baseline, and who let them back in.
    Admission {
        model: String,
        /// `"blocked"` or `"cleared"`.
        action: String,
        /// The blessed baseline's identity that refused.
        reference: String,
        /// `PROVENANCE_OPERATOR` on a clearing; the drift watch's own
        /// name when the block was set.
        provenance: String,
    },
    /// One drift comparison, exactly as the gate ran it (drift-watch design
    /// §4).
    ///
    /// **How many rows a boot writes is a family, not a fixed count.** A model
    /// gets two *first-reading* rows per boot — `comparison` is `"step"`
    /// (against last boot's profile) or `"cumulative"` (against the blessed
    /// baseline) — plus **at most one confirm row per comparison that read
    /// `"drift"`**, since a drift reading is the one outcome §4 says must be
    /// re-tested before it means anything. So: two rows on an ordinary boot,
    /// three when exactly one comparison read drift, four when both did. A
    /// confirm that never produced a re-diff at all (its probe failed, or its
    /// document could not be retained) writes no `Drift` row — it journals a
    /// `Degraded` instead, and the first reading stands. A probe failure's
    /// `Degraded` names the model and the comparison; a retention failure's
    /// names the staging document it could not retain, whose filename carries
    /// the model but not the comparison.
    ///
    /// The row is built to be **re-runnable and verifiable**, and to contain
    /// no transcribed measurements:
    ///
    /// - `outcome` is a named verdict — never a score, and never a number
    ///   copied out of either profile: a value that looks like a measurement,
    ///   transcribed, is how transcription errors become evidence. Which
    ///   vocabulary it draws on is what tells the two row kinds apart:
    ///   - a **first-reading row** carries the gate's own verdict —
    ///     `"within-noise"`, `"drift"`, `"not-comparable"`,
    ///     `"instrument-changed (<ref> -> <cur>)"`, `"unmeasured: <why>"`,
    ///     `"infra: <what>"`.
    ///   - a **confirm row** carries the verdict that reading finally
    ///     *settled* on, not the raw re-diff outcome underneath it —
    ///     `"confirmed"`, `"transient"`, or
    ///     `"unconfirmed: <named re-diff outcome>"`, where the text after the
    ///     colon is itself one of the gate's spellings above. Carrying the raw
    ///     word instead would make a confirmed regression read `"drift"`,
    ///     indistinguishable from the first reading that triggered it, and
    ///     would spell a *transient* — a finding in its own right — as
    ///     `"within-noise"`, the same word a clean boot gets.
    ///
    ///   Like [`Event::Blessed::provenance`], this is read by **prefix**, not
    ///   by equality against a closed set: `"unmeasured: "`, `"infra: "` and
    ///   `"unconfirmed: "` all carry free prose after the colon, and
    ///   `"instrument-changed"` carries both instrument identities in its
    ///   parenthetical.
    /// - `reference_path` / `current_path` name the two documents, so anyone
    ///   can re-run the identical `assay diff` by hand. A confirm row names
    ///   the same reference and the confirm's own retained document — the pair
    ///   its re-diff actually compared, not the first reading's pair.
    /// - `exit_code` is what `assay diff --gate` reported (on a confirm row,
    ///   what the *re-diff* reported), `None` when no diff ran at all
    ///   (precheck refused, a side was unmeasured, the spawn failed or the
    ///   child was killed by a signal). `None`, not `-1` and not `0`: an
    ///   absent code is not a zero one.
    /// - `reference_sha` / `current_sha` are the sha256 of each file's
    ///   **bytes**, taken when the gate read them — the same claim
    ///   [`Event::Blessed::sha`] makes, so `sha256sum` on either path checks
    ///   the row against the file. `None` when that side was never read.
    Drift {
        model: String,
        comparison: String,
        outcome: String,
        reference_path: String,
        current_path: String,
        exit_code: Option<i32>,
        reference_sha: Option<String>,
        current_sha: Option<String>,
    },
    /// One coverage verdict on a candidate GGUF offered as a substitute for
    /// `model` (swap-candidate seam design §4). Advisory: nothing in this
    /// daemon reads it back, and no admission decision derives from it — the
    /// row IS the deliverable.
    ///
    /// Built to be **re-runnable and verifiable from the row alone**, and to
    /// carry no transcribed measurement, the same two rules
    /// [`Event::Drift`] is built to:
    ///
    /// - `candidate_gguf_sha` is the full-file sha256 of the candidate's
    ///   **weights**, the same digest the pager's model registry keeps (and
    ///   `/status` renders), so the row names *which* file was measured and
    ///   not merely which path it sat at.
    /// - `floor_path`/`floor_sha` and
    ///   `candidate_profile_path`/`candidate_profile_sha` are the two
    ///   documents `assay cover` was handed and the sha256 of each one's
    ///   **bytes** — so `sha256sum` on either path checks the row, and
    ///   `assay cover <floor_path> <candidate_profile_path>` re-runs exactly
    ///   the comparison this row reports — and keeps doing so, because the
    ///   candidate's document is *retained content-named* (design §4 step 3)
    ///   rather than left at the one staging path every candidate for this
    ///   model would share, so a later job cannot overwrite the evidence an
    ///   earlier row points at. `candidate_profile_sha` is `None` only when
    ///   those bytes could not be re-read after they were retained; `None`,
    ///   never an empty string.
    /// - `exit_code` is what `assay cover` reported, `None` when it never
    ///   answered (spawn failure, or a child killed by a signal, which leaves
    ///   no code at all). `None`, not `-1` and not `0`.
    /// - `outcome` is a named verdict, never a score and never a number
    ///   copied out of either document. Like [`Event::Blessed::provenance`]
    ///   and [`Event::Drift::outcome`] it is read by **prefix**, not by
    ///   equality against a closed set: `"covered"`, `"not-covered"`,
    ///   `"incomplete"`, `"refused"` — which carries assay's own reason after
    ///   a colon when it gave one, because exit 2 is also what `argparse`
    ///   answers for `invalid choice: 'cover'` and a stale assay must not
    ///   read as a considered refusal about the candidate — and `"infra: …"`
    ///   for a cover that could not answer at all.
    ///
    /// **A row exists only where a comparison was really attempted.** A probe
    /// that failed produces no second document, so there is nothing to
    /// compare and nothing to record here: that path journals
    /// [`Event::Degraded`] naming the model and the probe's own words, and no
    /// verdict is invented.
    SwapCandidate {
        /// The configured model whose role the candidate would take — never
        /// the scratch identity the probe ran under, which exists only for
        /// the length of the job.
        model: String,
        candidate_gguf_sha: String,
        floor_path: String,
        floor_sha: String,
        candidate_profile_path: String,
        candidate_profile_sha: Option<String>,
        exit_code: Option<i32>,
        outcome: String,
    },
    /// The memory organ's per-task stamp (memory-organ design
    /// `docs/superpowers/specs/2026-08-26-memory-organ-design.md` §4).
    /// Written **once per spawned task, before its first step** — including
    /// tasks that ran with the organ off, which is the point: §4's
    /// lens-travels-with-verdict rule is satisfied by *record*, so a replay
    /// must be able to say of every task whether memory could have spoken to
    /// it, not only of the tasks where it did.
    ///
    /// `mode` is read by equality against a closed set of three spellings,
    /// unlike [`Event::Blessed::provenance`]'s prefix family:
    ///
    /// - `"off"` — no organ was offered to this task, or the operator's
    ///   config switch is off, or the store failed to load at boot (design
    ///   §7's disabled-with-reason). `episode_id` is `None` and
    ///   `candidates_checked` is `0`, because retrieval never ran.
    /// - `"silent"` — retrieval ran and produced nothing to inject.
    ///   `candidates_checked` is how many stored episodes shared this goal's
    ///   hash and were examined, survivor or not, so a `0` ("this goal is a
    ///   stranger") is distinguishable from an `N` ("N candidates were
    ///   checked and every one was disqualified") — the difference between
    ///   an empty store and a drifted workspace.
    /// - `"injected"` — `episode_id` names the one episode this task's
    ///   prompt carried (design §3: at most one is ever injected).
    ///
    /// A `"silent"` stamp's `episode_id` is `None`, never `Some("")`: an
    /// absent injection must not read as an injection of nothing.
    MemoryStamp {
        id: AgentId,
        task_id: String,
        mode: String,
        episode_id: Option<String>,
        candidates_checked: u32,
        /// Refalsify-on-exact's verdict for this retrieval (refalsify spec
        /// `docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md`
        /// §4): `None` when the flag is off, memory is off, or nothing was
        /// retrieved; `Some("passed" | "failed" | "skipped_ungranted" |
        /// "inconclusive")` when a hit was probed or skipped. Additive —
        /// absent-key rows replay as `None`, which is the truth of every
        /// pre-refalsify stamp. A `"failed"` stamp is always accompanied by
        /// an ordinary [`Event::MemoryContradicted`] citing the same task.
        #[serde(default)]
        refalsify: Option<String>,
    },
    /// A verified task minted (or refreshed) an episode (design §2). Paired
    /// with the task's own [`Event::MemoryStamp`] by `task_id`, which is
    /// what makes the task → store evidence trail walkable in both
    /// directions (design §4). A repeat that verifies re-mints the *same*
    /// `episode_id` with a fresh `minted_at` (design §5), so two rows naming
    /// one episode are a refresh, not a duplicate.
    MemoryMint {
        id: AgentId,
        task_id: String,
        episode_id: String,
    },
    /// An episode was refuted, so its stored status became `contradicted`
    /// and it will never be injected again. One row, two ways to earn it:
    ///
    /// - **Passive falsification** (memory-organ design §5): the task that
    ///   received the injection then failed its own verification. The organ
    ///   only reads that outcome; it initiates nothing to produce it.
    /// - **Refalsification** (refalsify-on-exact design
    ///   `docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md`
    ///   §2.3): before injecting, the worker re-ran the episode's own stored
    ///   verification command under the incoming task's grant and it exited
    ///   cleanly nonzero. That task then runs memory-silent, so its
    ///   `Event::MemoryStamp` says `mode: "silent"` with
    ///   `refalsify: Some("failed")` — which is also how a replay tells the
    ///   two paths apart, the passive one always pairing with an `injected`
    ///   stamp instead.
    ///
    /// `task_id` is the accusing task either way — the one that received the
    /// injection and failed, or the one whose probe refuted it — and is also
    /// the `contradicted_by` value written into the store row, so the
    /// journal and the store always name the same accuser.
    MemoryContradicted {
        id: AgentId,
        task_id: String,
        episode_id: String,
    },
}

/// `Event::CodecFixture::expect`'s serde default (G5 design doc §4): every
/// fixture was patch-class before `expect` existed, so an absent key means
/// `"patch"`, not `"refuse"` — the direction that keeps old journals
/// replaying as exactly what they always were.
fn default_expect_patch() -> String {
    "patch".to_string()
}

/// `Event::TaskStep::rung`'s serde default (window-ladder design doc §6):
/// every step's prompt was sent at full scope before the ladder existed, so
/// an absent key means rung 1, not rung 0 — a rung the ladder never had, and
/// the value a bare `#[serde(default)]` would silently replay old rows as.
fn default_rung_one() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PagerOpKind {
    SuspendSave,
    ResumeLoad,
    EvictSave,
}

pub struct Journal {
    writer: BufWriter<File>,
}

/// One journal row as written: the event, plus the writer's own wall-clock
/// stamp. `epoch_ms` is milliseconds since the Unix epoch (the `_ms` naming
/// the schema already uses for durations), read from the system clock at
/// append time — it records *when the writer wrote*, not what happened, so it
/// is a **row** property and never an `Event` field. It exists so a row can
/// be correlated with clocks outside the journal (GPU sample logs, daemon
/// stderr, an operator's notes): the swap-candidate acceptance found a VRAM
/// dip nobody could tie to a row because no row said when (bA2/F2). It is the
/// writer's clock reading, never a number transcribed from a measured
/// document, so the no-transcribed-measurements law
/// ([`Event::Drift`]'s doc) is untouched.
///
/// Two properties a correlating reader must know:
///
/// - **The stamp is wall clock, not monotonic.** The system clock can step
///   backwards (NTP), so `epoch_ms` is not guaranteed non-decreasing down the
///   file — file order is the row order, always; never sort by the stamp or
///   difference two stamps for an elapsed time. In-journal durations are the
///   `duration_ms` fields, measured separately and unaffected by clock steps.
/// - **The stamp is the append instant** — the *end* of whatever the row
///   describes. A row carrying `duration_ms` covers roughly
///   `[epoch_ms - duration_ms, epoch_ms]`, not the stamp's instant: a
///   22-second `ModelLoaded` is stamped 22 seconds after the load began.
///
/// Rows written before 2026-08-20 carry no stamp; [`replay`] accepts both,
/// and returns the events without the stamp either way — the raw JSONL is
/// the correlation surface. (A Rust consumer that one day needs the stamp
/// gets a `replay_rows() -> (stamp, event)` beside [`replay`], never a new
/// `Event` field — the cheap move would put the writer's clock inside the
/// what-happened record.) `#[serde(flatten)]` makes the row one flat object;
/// what keeps the `event` tag *first* is field order — `event` is declared
/// before `epoch_ms` and serde emits in declaration order — so rows stay
/// greppable as `{"event":"…` (the layout is pinned by
/// `an_appended_row_carries_a_bounded_epoch_ms_stamp`).
#[derive(serde::Serialize)]
struct Row<'a> {
    #[serde(flatten)]
    event: &'a Event,
    epoch_ms: u64,
}

impl Journal {
    pub fn open(path: &Path) -> std::io::Result<Journal> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Journal {
            writer: BufWriter::new(file),
        })
    }

    /// Appends one event as a single JSON line — the event's fields plus the
    /// row's `epoch_ms` wall-clock stamp ([`Row`]) — flushed immediately.
    /// Crash-durable enough for Phase 1: the write is flushed to the OS
    /// after every append, so at most the in-flight append can be lost.
    pub fn append(&mut self, e: &Event) -> std::io::Result<()> {
        // A clock before 1970 has no honest millisecond count; 0 is visibly
        // absurd rather than silently plausible, and the row still lands. A
        // count past u64 milliseconds saturates rather than silently
        // truncating (the daemon's own `millis` convention).
        let epoch_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        let line = serde_json::to_string(&Row { event: e, epoch_ms })?;
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }
}

/// Replays every line of the journal at `path` back into events.
///
/// Any unparseable line is a hard error — a corrupt journal must fail
/// loudly rather than silently skip events (project law 7).
///
/// The row's `epoch_ms` stamp ([`Row`]) is deliberately not returned: replay
/// reconstructs *what happened*, and the stamp records when the writer wrote.
/// A consumer correlating rows against wall clocks reads the raw JSONL, where
/// the stamp lives. Rows written before the stamp existed (2026-08-20) carry
/// no `epoch_ms` key and replay identically — pinned per committed journal by
/// `committed_g2_journal_still_replays`.
pub fn replay(path: &Path) -> std::io::Result<Vec<Event>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    reader
        .lines()
        .map(|line| {
            let line = line?;
            serde_json::from_str(&line).map_err(std::io::Error::from)
        })
        .collect()
}

pub fn sha256_hex(s: &str) -> String {
    sha256_hex_bytes(s.as_bytes())
}

/// The same digest over raw bytes, for callers hashing a file rather than a
/// prompt (the drift watch's content-addressed profiles). One implementation,
/// so a hex-formatting difference can never make the daemon's file digests
/// and the journal's prompt digests disagree about what sha256 looks like.
pub fn sha256_hex_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
