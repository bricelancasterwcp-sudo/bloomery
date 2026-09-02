//! Fixtures shared by the `pager_*` integration tests.
//!
//! Collected here on 2026-09-01 (carried-debt slice D) from six near-identical
//! copies that had drifted apart. Measured before unifying: `pager_in` existed
//! in **three** distinct shapes, `write_gguf` in three, `meta` in two — so
//! this was not one helper copied six times, it was one helper *forked* six
//! times, which is the more expensive kind of duplication because each fork
//! looks correct in isolation.
//!
//! Every signature below is the **superset** of the variants it replaces, so
//! no caller loses a capability and none gains behaviour it did not have:
//!
//! | helper       | variants replaced | superset chosen |
//! |--------------|-------------------|-----------------|
//! | `ok`         | 1 (identical)     | unchanged |
//! | `fresh_dir`  | 1 (identical)     | unchanged |
//! | `meta`       | `meta()` / `meta(w)` | `meta(w)`; the no-arg form hard-coded `1000` |
//! | `write_gguf` | `(dir, contents)` / `(dir, name)` / `(dir, name, contents)` | `(dir, name, contents)` |
//! | `pager_in`   | `Pager` / `(Pager, jpath)` / `(Pager, jpath, imgdir)` | the 3-tuple |

use std::path::{Path, PathBuf};

use bloomery_core::journal::Journal;
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::pager::Pager;
use bloomery_substrate::{fake::FakeSubstrate, Reply};

/// A scripted successful reply. Identical in all five files that carried it.
pub fn ok(text: &str) -> Reply {
    Reply {
        text: text.into(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 3,
    }
}

/// The shared "qwen" geometry — 28 layers, 4 kv-heads, 128 head-dim, so
/// `kv_per_token = 57_344` — parameterized on `weights_bytes`.
///
/// The no-arg variant this replaces hard-coded `weights_bytes: 1000`; its
/// call sites now pass that literal, which is strictly more honest since the
/// number matters to the reservation arithmetic those tests assert on.
pub fn meta(weights_bytes: u64) -> bloomery_core::gguf::GgufMeta {
    bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes,
        value_length: None,
        recurrent_state_bytes: 0,
    }
}

/// A clean scratch dir per test, so runs never share journals or images.
///
/// Deliberately keeps the fixed-name-and-remove form rather than adding a
/// pid/seq suffix: a stable name means each run *reclaims* its predecessor's
/// directory instead of leaving a new one behind, which is the opposite of
/// the `/tmp` accumulation this project's carried debt already records.
/// Callers pass names that are unique per test.
pub fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes fixture weights and returns the path.
pub fn write_gguf(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let gguf = dir.join(name);
    std::fs::write(&gguf, contents).unwrap();
    gguf
}

/// A pager over a fake substrate with `replies` scripted and a constant
/// free-VRAM probe (the fixture's static reservation budget).
///
/// Returns the journal path and image dir alongside the pager. Two of the
/// three variants this replaces discarded one or both; a caller that does not
/// want them writes `let (p, _, _) = ...`, which is cheaper than three
/// functions that differ only in what they throw away.
pub fn pager_in(
    dir: &Path,
    replies: usize,
    free_vram: Option<u64>,
) -> (Pager<FakeSubstrate>, PathBuf, PathBuf) {
    let jpath = dir.join("j.jsonl");
    let journal = Journal::open(&jpath).unwrap();
    let imgdir = dir.join("img");
    let images = ImageStore::new(&imgdir).unwrap();
    let mut fake = FakeSubstrate::new();
    for _ in 0..replies {
        fake.script_reply(ok("r"));
    }
    let p = Pager::new(fake, journal, images, Box::new(move || free_vram));
    (p, jpath, imgdir)
}
