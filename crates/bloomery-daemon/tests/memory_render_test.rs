//! The memory block's rendering and its injection seam (memory-organ Task 6;
//! spec `docs/superpowers/specs/2026-08-26-memory-organ-design.md` §4).
//!
//! Three properties, one per brief test, plus a fourth the brief's three
//! cannot reach (see its own doc comment):
//!
//! 1. **The block's bytes are pinned.** `render_memory_block`'s output is a
//!    literal here, not a call to the renderer it checks — a golden computed
//!    from its own subject would agree with any mutation of it, exactly what
//!    `task_render_test.rs`'s own module docs warn against.
//! 2. **Memory-off is byte-identical.** The whole slice rests on this: the
//!    organ is "advisory and inert by default … its total failure must be
//!    indistinguishable from memory-off" (spec §7), and every G4/G5 verdict
//!    in the ledger was measured against prompts rendered before this file
//!    existed. This test drives a REAL `run_task` under all four lenses and
//!    byte-compares the prompt the substrate actually received against
//!    [`render_task_prompt`], the public face the goldens pin.
//! 3. **Injection lands in the specified place** — after the goal, before
//!    the grant section (spec §4).
//!
//! **Why a local `RecordingSubstrate` rather than `FakeSubstrate`.**
//! `FakeSubstrate::ctx_history` accumulates every prompt a context has seen
//! into one concatenated string, which is enough to assert a prefix but not
//! enough to byte-compare *one* turn's prompt: the equality in test 2 is the
//! whole point, and a concatenation would silently pass a renderer that
//! appended trailing bytes to the last turn. Recording each prompt
//! separately makes `prompts[0] == render_task_prompt(..)` an exact
//! statement. Mirrors `task/registry.rs`'s local `PanicSubstrate` pattern
//! (:434) — a purpose-built `Substrate` living in the test that needs it.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bloomery_core::action::PatchCodec;
use bloomery_core::gguf::GgufMeta;
use bloomery_core::grant::Grant;
use bloomery_core::journal::Journal;
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::memory::record::{
    CitedFile, EpisodeRecord, Fingerprint, RunEvidence, StoredPatch,
};
use bloomery_daemon::memory::render::render_memory_block;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::task_loop::render_task_prompt;
use bloomery_daemon::task::{run_task, ExecBounds, TaskSpec, TaskStatus};
use bloomery_substrate::{CtxHandle, ModelHandle, Reply, Substrate, SubstrateError};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A fresh, per-test tempdir — PID + atomic counter, so parallel test
/// threads in one `cargo test` process never collide. Copied from
/// `task/registry.rs`'s test helpers.
fn fresh_dir(tag: &str) -> PathBuf {
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-memrender-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn meta() -> GgufMeta {
    GgufMeta {
        arch: "qwen2".into(),
        layers: 4,
        attention_layers: 4,
        kv_heads: 2,
        head_dim: 32,
        training_ctx: 65536,
        weights_bytes: 1000,
        recurrent_state_bytes: 0,
    }
}

/// A grant over `dir` that also carries a runnable command prefix, so the
/// envelope-v4 grant line renders a real `Granted commands:` line rather
/// than the `none` fallback — the section the memory block must land
/// *before*.
fn ok_grant(dir: &Path) -> Grant {
    let sb = std::fs::canonicalize(dir).unwrap();
    Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}"],"commands":[["python3","-m","unittest"]]}}"#,
        s = sb.display()
    ))
    .unwrap()
}

/// A [`Substrate`] that records every prompt `infer` is handed and answers
/// from a FIFO script. See this module's docs for why the recording is
/// per-prompt rather than `FakeSubstrate`'s concatenated history.
struct RecordingSubstrate {
    prompts: Arc<Mutex<Vec<String>>>,
    replies: VecDeque<String>,
}

impl RecordingSubstrate {
    fn new(replies: &[&str]) -> (Self, Arc<Mutex<Vec<String>>>) {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let sub = Self {
            prompts: Arc::clone(&prompts),
            replies: replies.iter().map(|s| (*s).to_string()).collect(),
        };
        (sub, prompts)
    }
}

impl Substrate for RecordingSubstrate {
    fn load_model(
        &mut self,
        _path: &Path,
        _n_gpu_layers: u32,
    ) -> Result<ModelHandle, SubstrateError> {
        Ok(1)
    }

    fn unload_model(&mut self, _m: ModelHandle) -> Result<(), SubstrateError> {
        Ok(())
    }

    fn create_context(
        &mut self,
        _m: ModelHandle,
        _n_ctx: u32,
    ) -> Result<CtxHandle, SubstrateError> {
        Ok(1)
    }

    fn destroy_context(&mut self, _c: CtxHandle) -> Result<(), SubstrateError> {
        Ok(())
    }

    fn infer(
        &mut self,
        _c: CtxHandle,
        prompt: &str,
        _max_tokens: u32,
        _stop: Option<&str>,
    ) -> Result<Reply, SubstrateError> {
        self.prompts.lock().unwrap().push(prompt.to_string());
        // Script exhaustion is a hard error, never a default reply — the
        // same rule `FakeSubstrate` states: a test must fail loudly rather
        // than pass on placeholder output.
        let text = self
            .replies
            .pop_front()
            .ok_or_else(|| SubstrateError::Infer("script exhausted".to_string()))?;
        Ok(Reply {
            text,
            prompt_tokens: Some(8),
            completion_tokens: Some(4),
            duration_ms: 1,
        })
    }

    fn save_state(&mut self, _c: CtxHandle) -> Result<Vec<u8>, SubstrateError> {
        Ok(Vec::new())
    }

    fn load_state(&mut self, _c: CtxHandle, _bytes: &[u8]) -> Result<(), SubstrateError> {
        Ok(())
    }
}

/// A pager over a [`RecordingSubstrate`], plus the agent id and the shared
/// prompt log.
fn build_pager(
    dir: &Path,
    replies: &[&str],
) -> (Pager<RecordingSubstrate>, String, Arc<Mutex<Vec<String>>>) {
    let (sub, prompts) = RecordingSubstrate::new(replies);
    let journal = Journal::open(&dir.join("pager.jsonl")).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut pager = Pager::new(sub, journal, images, Box::new(|| Some(1024 * 1024 * 1024)));
    let gguf = dir.join("m.gguf");
    std::fs::write(&gguf, b"fake weights").unwrap();
    pager.register_model("m", &gguf, meta(), None).unwrap();
    let info = pager.create_agent("m", 100, None, 1_000_000).unwrap();
    (pager, info.id, prompts)
}

const GOAL: &str = "fix the off-by-one in total() so the sum includes the last element";

/// One `done` turn: the shortest script that reaches exactly one `infer`,
/// so `prompts[0]` is the whole of what the loop rendered.
const DONE_TURN: &str = "<action verb=\"done\">\nall set\n</action>";

/// The episode every test in this file renders from — one `search_replace`
/// patch and one verifying run.
fn episode() -> EpisodeRecord {
    EpisodeRecord {
        episode_id: "ep-1".into(),
        goal_hash: "deadbeef".into(),
        goal_text: GOAL.into(),
        cited_files: vec![CitedFile {
            path: "/w/total.py".into(),
            fingerprint: Fingerprint::Sha256("abc123".into()),
        }],
        landed_patches: vec![StoredPatch {
            path: "/w/total.py".into(),
            codec: "search_replace".into(),
            body: "<<<<<<< SEARCH\n    return sum(xs[:-1])\n=======\n    return sum(xs)\n>>>>>>> REPLACE".into(),
        }],
        run_evidence: RunEvidence {
            argv: vec!["python3".into(), "-m".into(), "unittest".into()],
            outcome: "exit 0".into(),
        },
        trajectory: vec!["read".into(), "patch".into(), "run".into(), "done".into()],
        minted_by_model: "m".into(),
        minted_by_envelope: "v4".into(),
        status: "verified".into(),
        contradicted_by: None,
        minted_at: 1_700_000_000_000,
    }
}

/// The block [`episode`] renders to, as a literal. This is this surface's
/// golden: deliberately pasted, never computed from the renderer under test.
const PINNED_BLOCK: &str = r#"[memory: verified prior attempt]
This exact goal was completed before against byte-identical starting files.
--- patch /w/total.py (search_replace)
<<<<<<< SEARCH
    return sum(xs[:-1])
=======
    return sum(xs)
>>>>>>> REPLACE
Verification: python3 -m unittest -> exit 0
[end memory]"#;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Spec §4: the injected block is "delimited, deterministically rendered",
/// quoted evidence only — no advice, no paraphrase, nothing model-written.
/// The literal below is the statement of that shape; determinism is asserted
/// as a second render of the same record, since a renderer that consulted a
/// clock or a hash-ordered map would differ between calls.
#[test]
fn render_memory_block_is_deterministic_and_pinned() {
    let e = episode();
    let block = render_memory_block(&e);
    assert_eq!(block, PINNED_BLOCK, "the memory block's bytes are pinned");
    assert_eq!(
        block,
        render_memory_block(&e),
        "the same record must render the same bytes every time"
    );
    // Spec §2's honesty rule, restated as a property of THIS surface: the
    // record's model-written and operator-display-only fields never reach
    // the model.
    assert!(
        !block.contains("deadbeef") && !block.contains("ep-1"),
        "internal identifiers are not evidence: {block:?}"
    );
    assert!(
        !block.contains(GOAL),
        "the goal is already the prompt's first block; the memory section \
         must not restate it: {block:?}"
    );
}

/// Spec §2 stores the landed patches "in step order" so an exact repeat can
/// replay them; a single-patch golden cannot tell an ordered render from a
/// reversed or deduplicated one, so this covers the multi-stanza case the
/// brief's three tests leave unreachable.
#[test]
fn every_landed_patch_renders_a_stanza_in_step_order() {
    let mut e = episode();
    e.landed_patches = vec![
        StoredPatch {
            path: "/w/a.txt".into(),
            codec: "whole_file".into(),
            body: "first".into(),
        },
        StoredPatch {
            path: "/w/b.txt".into(),
            codec: "whole_file".into(),
            body: "second".into(),
        },
    ];
    let block = render_memory_block(&e);
    assert_eq!(
        block,
        "[memory: verified prior attempt]\n\
         This exact goal was completed before against byte-identical starting files.\n\
         --- patch /w/a.txt (whole_file)\n\
         first\n\
         --- patch /w/b.txt (whole_file)\n\
         second\n\
         Verification: python3 -m unittest -> exit 0\n\
         [end memory]",
        "one stanza per landed patch, in order"
    );
}

/// **The byte-identity law** (spec §4: memory-off "renders to the empty
/// string … byte-identical to today"). Drives the REAL `run_task` with
/// `memory_block: None` under every lens and byte-compares the prompt the
/// substrate received against [`render_task_prompt`] — the public face
/// `task_render_test.rs`'s literal goldens pin. Equality here plus those
/// goldens is what makes "memory-off did not move a single byte" a proof
/// rather than a claim.
#[test]
fn absent_memory_renders_byte_identical_prompts() {
    for envelope in [
        EnvelopeLens::V1,
        EnvelopeLens::V2,
        EnvelopeLens::V3,
        EnvelopeLens::V4,
    ] {
        let dir = fresh_dir("absent");
        let cwd = std::fs::canonicalize(&dir).unwrap();
        let grant = ok_grant(&dir);
        let (mut pager, agent_id, prompts) = build_pager(&dir, &[DONE_TURN]);
        let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
        let spec = TaskSpec {
            goal: GOAL.to_string(),
            grant: grant.clone(),
            budget_tokens: 1_000_000,
            max_steps: 3,
            cwd,
            patch_codec: PatchCodec::SearchReplace,
            bounds: ExecBounds::default(),
            mutating_verbs: true,
            envelope,
            memory_block: None,
            window_ladder: false,
        };

        let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
        assert_eq!(result.status, TaskStatus::Done, "{result:?}");

        let prompts = prompts.lock().unwrap();
        assert_eq!(
            prompts.len(),
            1,
            "a one-turn task renders exactly one prompt"
        );
        assert_eq!(
            prompts[0],
            render_task_prompt(
                GOAL,
                PatchCodec::SearchReplace,
                envelope,
                grant.commands(),
                ""
            ),
            "memory-off rendering must be byte-identical to the public face ({envelope:?})"
        );
        assert!(
            !prompts[0].contains("[memory:"),
            "a memory-off task must show the model nothing at all: {:?}",
            prompts[0]
        );

        drop(prompts);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Spec §4: the memory block is rendered "immediately after the goal block
/// and before the grant section". Both halves are asserted positionally, so
/// a renderer that put the block after the verb card, or after the
/// transcript, fails.
#[test]
fn injected_memory_appears_after_goal_before_grant_line() {
    let dir = fresh_dir("injected");
    let cwd = std::fs::canonicalize(&dir).unwrap();
    let grant = ok_grant(&dir);
    let (mut pager, agent_id, prompts) = build_pager(&dir, &[DONE_TURN]);
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let block = render_memory_block(&episode());
    let spec = TaskSpec {
        goal: GOAL.to_string(),
        grant,
        budget_tokens: 1_000_000,
        max_steps: 3,
        cwd,
        patch_codec: PatchCodec::SearchReplace,
        bounds: ExecBounds::default(),
        mutating_verbs: true,
        envelope: EnvelopeLens::V4,
        memory_block: Some(block.clone()),
        window_ladder: false,
    };

    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
    assert_eq!(result.status, TaskStatus::Done, "{result:?}");

    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), 1);
    let prompt = &prompts[0];

    assert!(
        prompt.starts_with(&format!("{GOAL}\n\n[memory: ")),
        "the block opens immediately after the goal: {prompt:?}"
    );
    assert!(
        prompt.contains(&format!("{block}\n\n")),
        "the block is rendered verbatim, one blank line before what follows: {prompt:?}"
    );

    let end_memory = prompt
        .find("[end memory]\n\n")
        .expect("the block's closing delimiter is present");
    let grant_at = prompt
        .find("Granted commands: python3 -m unittest")
        .expect("the v4 grant line is present");
    let card_at = prompt
        .find("# Action verbs")
        .expect("the verb card is present");
    assert!(
        end_memory < grant_at && grant_at < card_at,
        "order must be goal, memory, grant, verb card: {prompt:?}"
    );

    drop(prompts);
    let _ = std::fs::remove_dir_all(&dir);
}
