//! Retrieval — spec §3: two-stage exact match (goal hash, then per-candidate
//! fingerprint + grant gates), the status gate, and single-survivor
//! selection.
//!
//! All three gates are silent-on-failure (spec §7: "Retrieval-time hashing
//! failure on any cited file: that candidate is a mismatch (silence), not an
//! error") — a disqualified candidate is simply excluded from the survivor
//! pool, never surfaced as an error to the caller. [`retrieve`] therefore
//! cannot fail; a task with memory enabled either gets one injected episode
//! or silence (`injected: None`), never a broken task (spec §7's headline:
//! "The organ being broken can only ever produce memory-off behavior").

use std::path::Path;

use bloomery_core::grant::Grant;
use bloomery_core::journal::sha256_hex_bytes;

use super::record::{goal_hash, CitedFile, EpisodeRecord, Fingerprint};
use super::store::MemoryStore;

/// The outcome of one retrieval attempt: at most one injected episode, plus
/// the number of candidates examined (survivor or not) — the pair the
/// journal stamp needs (spec §4: "what was injected ... or `silent` with
/// `candidates_checked`").
pub struct Retrieval {
    pub injected: Option<EpisodeRecord>,
    pub candidates_checked: u32,
}

/// Retrieve the best matching verified episode for `goal`, gated by `grant`.
///
/// `_cwd` is accepted for interface symmetry with the capture seam but is
/// unused here: stored `cited_files[].path` values are already
/// canonicalize-then-`display()` absolute forms minted by the capture seam
/// (Task 3's carry-note), so retrieval never needs to re-absolutize a cited
/// path against a task cwd.
pub fn retrieve(store: &MemoryStore, goal: &str, grant: &Grant, _cwd: &Path) -> Retrieval {
    let hash = goal_hash(goal);
    let mut candidates_checked: u32 = 0;
    let mut survivors: Vec<&EpisodeRecord> = Vec::new();

    for candidate in store.candidates(&hash) {
        // `candidates_checked` counts every candidate examined, survivor or
        // not (brief, Produces).
        candidates_checked += 1;

        // Status gate first: cheapest check of the three (spec §3's fourth
        // bullet), so a non-verified candidate never pays for the file IO
        // the fingerprint/grant gate below does. All three gates are AND'd
        // together, so checking status first changes nothing about which
        // candidates survive — only how much work a disqualified one costs.
        if candidate.status != "verified" {
            continue;
        }

        // Fingerprint + grant gate (spec §3): every cited file must match,
        // silently — any mismatch or IO error disqualifies the candidate,
        // never errors retrieval (spec §7).
        if candidate
            .cited_files
            .iter()
            .all(|cf| cited_file_matches(cf, grant))
        {
            survivors.push(candidate);
        }
    }

    // Selection: greatest `minted_at` wins; ties break on `episode_id`
    // descending for determinism (spec §3: "At most one episode is
    // injected: the most recently verified survivor").
    let injected = survivors
        .into_iter()
        .max_by(|a, b| {
            a.minted_at
                .cmp(&b.minted_at)
                .then_with(|| a.episode_id.cmp(&b.episode_id))
        })
        .cloned();

    Retrieval {
        injected,
        candidates_checked,
    }
}

/// One cited file's fingerprint + grant gate (spec §3).
fn cited_file_matches(cf: &CitedFile, grant: &Grant) -> bool {
    let path = Path::new(&cf.path);
    match &cf.fingerprint {
        Fingerprint::Sha256(expected_hex) => {
            // `check_read` IS the grant gate for an existing file: it
            // returns the canonical path only if `path` resolves inside a
            // granted read root (spec §3 — "every cited path must fall
            // inside the requesting grant's read roots. Memory must never
            // show an agent bytes its own grant could not have read").
            let Ok(canon) = grant.check_read(path) else {
                return false;
            };
            let Ok(bytes) = std::fs::read(&canon) else {
                return false;
            };
            sha256_hex_bytes(&bytes) == *expected_hex
        }
        Fingerprint::Absent => {
            // A nonexistent path cannot be canonicalized, so `check_read`
            // has nothing to resolve against for the "does not exist" case
            // — per spec §3 the grant gate still applies to an `absent`
            // expectation, so the honest check here is lexical: the path
            // must sit under a declared read root via `Path::starts_with`.
            // Stored `cited_files[].path` values are already canonical-
            // absolute (Task 3's capture seam), so this lexical check is
            // exactly the containment `check_read` would have confirmed had
            // the file existed — not a traversal shortcut.
            !path.exists() && grant.read_roots().iter().any(|root| path.starts_with(root))
        }
    }
}
