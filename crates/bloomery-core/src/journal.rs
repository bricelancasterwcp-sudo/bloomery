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
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
