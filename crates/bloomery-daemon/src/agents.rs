//! Per-agent state: the resident/suspended/fresh table and the KV image
//! store that backs suspend/resume across it.
//!
//! `ImageStore` holds a RAM tier (fast, volatile) and an NVMe spill tier
//! (slower, survives a `spill`). Every image is tagged with the digest of
//! the model it belongs to: [`ImageStore::take`] treats a digest mismatch
//! as *invalidation*, not an error (the model on disk changed since the
//! image was saved, so the image is simply stale), which is why
//! [`ImageFetch::StaleDigest`] is a distinct, non-error variant from
//! [`ImageFetch::Missing`].
//!
//! Every spilled image also records its exact byte length at spill time.
//! `take` re-checks that length against what it actually reads off disk
//! before handing the bytes to a caller: a substrate's `set_data_ext` call
//! returns a bare `bool`, so an over-long or truncated KV image can restore
//! as a *bogus success* at that layer. This is the one place upstream of
//! the substrate that can catch that kind of corruption, so a length
//! mismatch is reported as [`ImageFetch::Corrupt`] rather than silently
//! handed off.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bloomery_core::journal::AgentId;

/// One agent's paging state.
pub struct Agent {
    pub id: AgentId,
    pub model: String,
    pub priority: u8,
    pub window: bloomery_core::geometry::Window,
    pub budget: bloomery_core::budget::Budget,
    /// `window.tokens * kv_per_token` for this agent's model.
    pub kv_bytes: u64,
    pub state: AgentState,
}

/// Where an agent's context currently lives.
pub enum AgentState {
    /// Loaded into the substrate, ready to infer.
    Resident { ctx: bloomery_substrate::CtxHandle },
    /// Evicted; its KV image (if any) lives in the [`ImageStore`].
    Suspended,
    /// Never inferred yet — nothing to page in or out.
    Fresh,
}

/// The live set of known agents, keyed by [`AgentId`].
#[derive(Default)]
pub struct AgentTable {
    agents: HashMap<AgentId, Agent>,
}

impl AgentTable {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    pub fn insert(&mut self, agent: Agent) {
        self.agents.insert(agent.id.clone(), agent);
    }

    pub fn get(&self, id: &str) -> Option<&Agent> {
        self.agents.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Agent> {
        self.agents.get_mut(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<Agent> {
        self.agents.remove(id)
    }

    /// Every currently-resident agent, in the shape the scheduler's
    /// residency planner (`bloomery_core::scheduler::plan_residency`)
    /// consumes. Task 12 does not yet track in-flight requests, so `busy`
    /// is always reported `false` here; a later task that adds request
    /// tracking is responsible for threading a real value through.
    pub fn residents(&self) -> Vec<bloomery_core::scheduler::Resident> {
        self.agents
            .values()
            .filter_map(|a| match &a.state {
                AgentState::Resident { .. } => Some(bloomery_core::scheduler::Resident {
                    id: a.id.clone(),
                    priority: a.priority,
                    kv_bytes: a.kv_bytes,
                    busy: false,
                }),
                _ => None,
            })
            .collect()
    }
}

/// A RAM-resident image: raw bytes tagged with the digest of the model they
/// were saved from.
struct RamEntry {
    digest: String,
    bytes: Vec<u8>,
}

/// An NVMe-spilled image: where it lives on disk, which model digest it was
/// saved under, and the byte length it was written with — the ground truth
/// `take` checks a disk read against before trusting it.
struct SpillEntry {
    digest: String,
    len: u64,
    path: PathBuf,
}

/// RAM tier + NVMe spill tier for agent KV images.
pub struct ImageStore {
    ram: HashMap<String, RamEntry>,
    spilled: HashMap<String, SpillEntry>,
    spill_dir: PathBuf,
}

/// The result of [`ImageStore::take`].
#[derive(Debug)]
pub enum ImageFetch {
    /// Found in the RAM tier.
    Ram(Vec<u8>),
    /// Found in the NVMe tier and read back intact.
    Nvme(Vec<u8>),
    /// An image exists for this id, but under a different model digest —
    /// invalidation, not an error. The stale entry is dropped.
    StaleDigest,
    /// No image at all for this id.
    Missing,
    /// An NVMe image was found under the expected digest, but the bytes
    /// read off disk don't match the length recorded at spill time.
    Corrupt,
}

impl ImageStore {
    /// Opens (creating if necessary) `spill_dir` as the NVMe tier.
    pub fn new(spill_dir: &Path) -> std::io::Result<ImageStore> {
        std::fs::create_dir_all(spill_dir)?;
        Ok(ImageStore {
            ram: HashMap::new(),
            spilled: HashMap::new(),
            spill_dir: spill_dir.to_path_buf(),
        })
    }

    /// Stores (or overwrites) `id`'s image in the RAM tier.
    ///
    /// Also invalidates any NVMe-spilled entry for `id`, best-effort
    /// deleting its backing file (errors, including the file already being
    /// gone, are ignored — this is cleanup, not the source of truth).
    /// Without this, a fresh RAM image followed by a spill under a new
    /// digest leaves the *old* spilled entry reachable: a later `take` for
    /// the old digest would find its file untouched, its recorded length
    /// still matching, and hand back a clean `Nvme(..)` — resurrecting an
    /// image the id's owner already moved past instead of reporting it
    /// gone.
    pub fn put_ram(&mut self, id: &str, digest: &str, bytes: Vec<u8>) {
        if let Some(old) = self.spilled.remove(id) {
            let _ = std::fs::remove_file(&old.path);
        }
        self.ram.insert(
            id.to_string(),
            RamEntry {
                digest: digest.to_string(),
                bytes,
            },
        );
    }

    /// Moves `id`'s RAM image to the NVMe tier, writing
    /// `{id}.{digest}.kvimg` under the spill dir. Any previously spilled
    /// file for `id` under a different digest is removed so re-spilling
    /// after a model change doesn't leak orphaned files.
    ///
    /// The RAM entry is only removed once the disk write has actually
    /// succeeded: if the write fails (out of space, I/O error, permission
    /// denied, ...) the only copy of the image must survive intact in RAM
    /// rather than being silently dropped along with the failed write.
    pub fn spill(&mut self, id: &str) -> std::io::Result<()> {
        let entry = self.ram.get(id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no RAM image to spill for {id}"),
            )
        })?;

        let filename = format!("{id}.{}.kvimg", entry.digest);
        let path = self.spill_dir.join(filename);
        std::fs::write(&path, &entry.bytes)?;

        // Write succeeded — only now is it safe to move the entry out of RAM.
        let entry = self.ram.remove(id).expect("checked present above");
        let len = entry.bytes.len() as u64;

        let old = self.spilled.insert(
            id.to_string(),
            SpillEntry {
                digest: entry.digest,
                len,
                path: path.clone(),
            },
        );
        if let Some(old) = old {
            if old.path != path {
                let _ = std::fs::remove_file(&old.path);
            }
        }
        Ok(())
    }

    /// Fetches `id`'s image, verified against `expect_digest`.
    ///
    /// Checks the RAM tier first, then NVMe. A digest mismatch in either
    /// tier drops the stale entry and returns [`ImageFetch::StaleDigest`].
    /// An NVMe hit is only trusted once the bytes actually read back match
    /// the length recorded at [`ImageStore::spill`] time; a mismatch
    /// returns [`ImageFetch::Corrupt`] instead of handing back a
    /// truncated or padded image.
    pub fn take(&mut self, id: &str, expect_digest: &str) -> ImageFetch {
        if let Some(entry) = self.ram.remove(id) {
            if entry.digest != expect_digest {
                return ImageFetch::StaleDigest;
            }
            return ImageFetch::Ram(entry.bytes);
        }

        if let Some(spilled) = self.spilled.remove(id) {
            if spilled.digest != expect_digest {
                let _ = std::fs::remove_file(&spilled.path);
                return ImageFetch::StaleDigest;
            }
            return match std::fs::read(&spilled.path) {
                Ok(bytes) if bytes.len() as u64 == spilled.len => ImageFetch::Nvme(bytes),
                Ok(_) => ImageFetch::Corrupt,
                Err(_) => ImageFetch::Missing,
            };
        }

        ImageFetch::Missing
    }
}

/// Cheap blob identity for a `.gguf` file: `sha256(first 1 MiB || file_len)`.
///
/// Reading only the first 1 MiB (rather than the whole, potentially
/// multi-gigabyte, weights file) keeps this fast enough to call on every
/// boot and every profile check; mixing in the total file length catches
/// the case where two files happen to share an identical first 1 MiB but
/// differ afterward.
pub fn model_digest(gguf: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let file_len = std::fs::metadata(gguf)?.len();
    let file = std::fs::File::open(gguf)?;
    let mut prefix = Vec::new();
    file.take(1024 * 1024).read_to_end(&mut prefix)?;

    let mut hasher = Sha256::new();
    hasher.update(&prefix);
    hasher.update(file_len.to_le_bytes());
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}
