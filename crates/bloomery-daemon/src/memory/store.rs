//! The JSONL episode store — event-sourced, append-only, last-writer-wins.
//!
//! See spec §6 (storage, operator surface, retention) in
//! docs/superpowers/specs/2026-08-26-memory-organ-design.md: every row is a
//! complete record (payload-is-the-record), a status change appends a new
//! full row for the same `episode_id` rather than mutating in place, and the
//! in-memory index — here, [`MemoryStore::episodes`], keyed by `episode_id`
//! — is rebuilt from the file at every [`MemoryStore::load`] and is never
//! itself the source of truth. This is the journal idiom
//! (`bloomery_core::journal`) applied one layer up: same append-only JSONL,
//! same `OpenOptions::new().create(true).append(true)` + one line + flush
//! discipline as `Journal::append` (`journal.rs:409`), no new dependency
//! (no SQLite).
//!
//! **Where this deliberately diverges from `journal::replay`:** a corrupt
//! journal line is a hard error (project law 7) because the journal is
//! evidence of what happened, and evidence that silently drops rows is
//! worse than no evidence. This store is advisory *memory* — a reader that
//! forgets one episode still lets every task run, just memory-off for that
//! one candidate — so per spec §6 a corrupt line is counted
//! (`MemoryCounts::parse_errors`, surfaced at `/status`) and skipped, never
//! fatal. A store the daemon can't parse must not become a store the daemon
//! can't boot with.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use super::record::{EpisodeRecord, StoredRow};

/// The live episode index plus enough bookkeeping to answer `/status`.
///
/// Fields are private: every mutation goes through [`MemoryStore::mint`],
/// [`MemoryStore::mark_contradicted`], or [`MemoryStore::delete`] so each
/// in-memory change is paired with the durable JSONL append that makes it
/// survive a reload (spec §6's event-sourced contract).
pub struct MemoryStore {
    path: PathBuf,
    /// Keyed by `episode_id`; last-writer-wins per id, as replayed from the
    /// file in append order.
    episodes: BTreeMap<String, EpisodeRecord>,
    parse_errors: u64,
}

/// The `/status` shape for the memory organ (spec §6): `enabled` is the
/// caller's concern (config, not store state) so it lives beside this in
/// `api_native.rs`, not on this struct.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct MemoryCounts {
    pub episodes: u64,
    pub verified: u64,
    pub contradicted: u64,
    pub parse_errors: u64,
}

impl MemoryStore {
    /// Loads the store at `path`, replaying every JSONL row in file order.
    ///
    /// A missing file is an **empty store**, not an error — first boot has
    /// no store yet, and that must not block the daemon (spec §6, §7: "Store
    /// unreadable at boot: organ reports itself disabled-with-reason ...
    /// tasks run memory-off" — a missing file is the routine case of that,
    /// not the exceptional one). The parent directory is created if
    /// missing so a later [`MemoryStore::mint`]'s append has somewhere to
    /// land.
    ///
    /// A line that fails to parse as [`StoredRow`] increments
    /// `parse_errors` and is skipped — see the module doc comment for why
    /// this deliberately differs from `journal::replay`'s hard error.
    pub fn load(path: &Path) -> io::Result<MemoryStore> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let mut episodes: BTreeMap<String, EpisodeRecord> = BTreeMap::new();
        let mut parse_errors: u64 = 0;

        match File::open(path) {
            Ok(file) => {
                for line in BufReader::new(file).lines() {
                    let line = line?;
                    match serde_json::from_str::<StoredRow>(&line) {
                        Ok(StoredRow::Episode(rec)) => {
                            episodes.insert(rec.episode_id.clone(), *rec);
                        }
                        Ok(StoredRow::Tombstone { episode_id }) => {
                            episodes.remove(&episode_id);
                        }
                        Err(_) => parse_errors += 1,
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // First boot: no file yet. Empty store, not an error.
            }
            Err(e) => return Err(e),
        }

        Ok(MemoryStore {
            path: path.to_path_buf(),
            episodes,
            parse_errors,
        })
    }

    /// Appends one JSON line and flushes — the `Journal::append` idiom
    /// (`journal.rs:409`) applied to this store's file, so a crash loses at
    /// most the in-flight append.
    fn append_row(&self, row: &StoredRow) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(row)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()
    }

    /// Tombstones `episode_id`: appends a durable `Tombstone` row and drops
    /// it from the live map. Shared by [`MemoryStore::delete`] (operator
    /// eviction) and retention (age-based eviction) — both are "this id is
    /// gone" in the same durable sense (spec §6).
    fn tombstone(&mut self, episode_id: &str) -> io::Result<()> {
        self.append_row(&StoredRow::Tombstone {
            episode_id: episode_id.to_string(),
        })?;
        self.episodes.remove(episode_id);
        Ok(())
    }

    /// Mints a new episode: appends the full `Episode` row, upserts the live
    /// map (last-writer-wins on `episode_id`), then enforces `max_episodes`
    /// by evicting until the distinct-id count is back at or under the cap.
    ///
    /// Eviction order (spec §6): contradicted-oldest-first, then
    /// verified-oldest-first; oldest by `minted_at`, ties broken by
    /// `episode_id` for determinism (so eviction order never depends on
    /// `BTreeMap` iteration incidental ordering — see
    /// [`MemoryStore::pick_eviction_victim`]).
    pub fn mint(&mut self, rec: EpisodeRecord, max_episodes: usize) -> io::Result<()> {
        self.append_row(&StoredRow::Episode(Box::new(rec.clone())))?;
        self.episodes.insert(rec.episode_id.clone(), rec);
        self.evict_for_retention(max_episodes)
    }

    fn evict_for_retention(&mut self, max_episodes: usize) -> io::Result<()> {
        while self.episodes.len() > max_episodes {
            match self.pick_eviction_victim() {
                Some(victim) => self.tombstone(&victim)?,
                None => break, // unreachable: len() > max_episodes >= 0 implies at least one entry
            }
        }
        Ok(())
    }

    /// Picks the next eviction victim: any contradicted episode outranks
    /// every verified one regardless of age (spec §6's
    /// "contradicted-oldest-first, then verified-oldest-first"), so the
    /// pool is "all contradicted" whenever one exists, else "everything".
    /// Within the chosen pool, oldest `minted_at` wins; ties break on
    /// `episode_id` so the order is deterministic across runs.
    fn pick_eviction_victim(&self) -> Option<String> {
        let any_contradicted = self.episodes.values().any(|e| e.status == "contradicted");
        self.episodes
            .values()
            .filter(|e| !any_contradicted || e.status == "contradicted")
            .min_by(|a, b| {
                a.minted_at
                    .cmp(&b.minted_at)
                    .then_with(|| a.episode_id.cmp(&b.episode_id))
            })
            .map(|e| e.episode_id.clone())
    }

    /// Marks `episode_id` contradicted: clones the current record, sets
    /// `status: "contradicted"` and `contradicted_by: Some(task_id)`,
    /// appends it as a full `Episode` row (event-sourced — a status change
    /// is a new row, never an in-place file edit), and updates the map.
    /// `Ok(false)` for an unknown id; no append, no mutation.
    pub fn mark_contradicted(&mut self, episode_id: &str, task_id: &str) -> io::Result<bool> {
        let Some(existing) = self.episodes.get(episode_id) else {
            return Ok(false);
        };
        let mut updated = existing.clone();
        updated.status = "contradicted".to_string();
        updated.contradicted_by = Some(task_id.to_string());
        self.append_row(&StoredRow::Episode(Box::new(updated.clone())))?;
        self.episodes.insert(episode_id.to_string(), updated);
        Ok(true)
    }

    /// The operator's `DELETE /memory/{id}` (spec §6): tombstones and
    /// removes `episode_id`. `Ok(false)` for an unknown id, no store
    /// mutation — the caller maps that to `404` (spec §7).
    pub fn delete(&mut self, episode_id: &str) -> io::Result<bool> {
        if !self.episodes.contains_key(episode_id) {
            return Ok(false);
        }
        self.tombstone(episode_id)?;
        Ok(true)
    }

    /// All live episodes whose `goal_hash` matches — the first stage of
    /// retrieval's exact match (spec §3). A linear scan is fine at
    /// `max_episodes` scale (brief, Produces).
    pub fn candidates(&self, goal_hash: &str) -> Vec<&EpisodeRecord> {
        self.episodes
            .values()
            .filter(|e| e.goal_hash == goal_hash)
            .collect()
    }

    /// Every live episode, in `episode_id` order (the map's own order) —
    /// used by `GET /memory` and by tests asserting on survivor sets.
    pub fn episodes(&self) -> impl Iterator<Item = &EpisodeRecord> {
        self.episodes.values()
    }

    /// The `/status` counts (spec §6): total live episodes, split by
    /// status, plus corrupt-line count from the last load.
    pub fn counts(&self) -> MemoryCounts {
        let mut verified = 0u64;
        let mut contradicted = 0u64;
        for e in self.episodes.values() {
            match e.status.as_str() {
                "verified" => verified += 1,
                "contradicted" => contradicted += 1,
                _ => {}
            }
        }
        MemoryCounts {
            episodes: self.episodes.len() as u64,
            verified,
            contradicted,
            parse_errors: self.parse_errors,
        }
    }
}
