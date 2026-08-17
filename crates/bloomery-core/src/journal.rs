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
    /// `Degraded` naming the model and the comparison, and the first reading
    /// stands.
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
}

/// `Event::CodecFixture::expect`'s serde default (G5 design doc §4): every
/// fixture was patch-class before `expect` existed, so an absent key means
/// `"patch"`, not `"refuse"` — the direction that keeps old journals
/// replaying as exactly what they always were.
fn default_expect_patch() -> String {
    "patch".to_string()
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

impl Journal {
    pub fn open(path: &Path) -> std::io::Result<Journal> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Journal {
            writer: BufWriter::new(file),
        })
    }

    /// Appends one event as a single JSON line, flushed immediately.
    /// Crash-durable enough for Phase 1: the write is flushed to the OS
    /// after every append, so at most the in-flight append can be lost.
    pub fn append(&mut self, e: &Event) -> std::io::Result<()> {
        let line = serde_json::to_string(e)?;
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }
}

/// Replays every line of the journal at `path` back into events.
///
/// Any unparseable line is a hard error — a corrupt journal must fail
/// loudly rather than silently skip events (project law 7).
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
