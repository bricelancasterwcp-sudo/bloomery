//! Refalsify: the lanes that never reach a verdict, and the oversized guard.
//!
//! An ungranted command and a demoted task skip probing entirely; a timed-out
//! probe and a signal death are `inconclusive` and inject anyway -- the
//! fail-open direction, deliberately, since an unrunnable probe is not
//! evidence against the episode. And an oversized episode is never probed at
//! all.
//!
//! Split out of `memory_refalsify_test.rs` on 2026-09-01 (slice D).

mod common;

use std::sync::{Arc, Mutex};

use bloomery_core::journal::{replay, sha256_hex_bytes};
use bloomery_daemon::memory::record::{
    episode_id, goal_hash, CitedFile, EpisodeRecord, Fingerprint, RunEvidence, StoredPatch,
};
use bloomery_daemon::memory::render::render_memory_block;
use bloomery_daemon::memory::MEMORY_BLOCK_MAX_BYTES;
use bloomery_daemon::task::{TaskRegistry, TaskStatus};
use common::memory::{
    build_pager, contradicted_ids, degraded_reasons, fresh_dir, memory_ctx, memory_prompts,
    spec_for, BEFORE,
};

use common::refalsify::{
    canary_exists, done_turn, drive, grant_with, mint, probe, sandbox, stamp_for, stored_status,
    untouched, CANARY_SCRIPT, GOAL, SH,
};

/// **Ungranted skip (refalsify spec §2.1).** Grants come from the incoming
/// REQUEST, not from the store, so a task whose grant does not cover the
/// episode's stored argv cannot probe it. The episode injects anyway
/// (refalsification upgrades trust where possible; it never shrinks reach
/// below the battery-passing behavior) and the stamp says
/// `skipped_ungranted`.
///
/// "No run was attempted" is observed directly, not inferred: the stored
/// command's only effect is the canary file, and the canary stays gone. That
/// is a stronger check than scanning the journal for execution rows — the
/// pre-check is specified to run BEFORE anything spawns, and a spawn that
/// happened and was then refused by `exec_run` would still have left the
/// file if it had run at all.
#[test]
fn an_ungranted_command_skips_and_injects() {
    let dir = fresh_dir("refalsify", "ungranted");
    let m = mint(&dir, CANARY_SCRIPT, 1);

    // Same read/write roots (retrieval's own grant gate still has to pass),
    // no command prefixes at all.
    let narrow = grant_with(&m.sb, &[]);
    assert!(
        narrow
            .check_command(&[SH[0].to_string(), SH[1].to_string(), "true".to_string()])
            .is_err(),
        "the fixture's second grant must genuinely not cover the stored argv"
    );

    let p = probe(
        &m,
        &dir,
        spec_for(GOAL, &narrow, &m.sb),
        memory_ctx(&dir, true, true),
    );
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert_eq!(
        p.stamp,
        (
            "injected".to_string(),
            Some(m.episode_id.clone()),
            1,
            Some("skipped_ungranted".to_string())
        ),
    );
    assert_eq!(
        memory_prompts(&dir),
        1,
        "an unprobed episode still injects — into the probed task's one turn"
    );
    assert!(
        !canary_exists(&m.sb),
        "the pre-check runs before anything spawns — nothing may have executed"
    );
    assert_eq!(p.stored, untouched(), "a skip touches no record");
    assert!(p.contradicted.is_empty(), "{:?}", p.contradicted);
}

/// **Demoted skip (refalsify spec §2.1).** The demotion boundary outranks
/// refalsification: a read-only task (`mutating_verbs == false`) may not
/// have commands executed at its moment, whatever its grant says — so it
/// takes the ungranted-class skip even though its grant here covers the
/// stored argv exactly.
///
/// Same canary observation as the ungranted test, and the grant is
/// deliberately the SAME one the mint ran under, so the only thing that can
/// produce the skip is the demotion.
#[test]
fn a_demoted_task_skips_even_with_a_covering_grant() {
    let dir = fresh_dir("refalsify", "demoted");
    let m = mint(&dir, CANARY_SCRIPT, 1);
    assert!(
        m.grant
            .check_command(&[
                SH[0].to_string(),
                SH[1].to_string(),
                CANARY_SCRIPT.to_string()
            ])
            .is_ok(),
        "the grant must cover the stored argv, or this test proves nothing"
    );

    let mut spec = spec_for(GOAL, &m.grant, &m.sb);
    spec.mutating_verbs = false;

    let p = probe(&m, &dir, spec, memory_ctx(&dir, true, true));
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert_eq!(
        p.stamp,
        (
            "injected".to_string(),
            Some(m.episode_id.clone()),
            1,
            Some("skipped_ungranted".to_string())
        ),
    );
    assert_eq!(
        memory_prompts(&dir),
        1,
        "a demoted task still receives — into its one turn"
    );
    assert!(
        !canary_exists(&m.sb),
        "a demoted task has no commands executed at its moment"
    );
    assert_eq!(p.stored, untouched());
    assert!(p.contradicted.is_empty(), "{:?}", p.contradicted);
}

/// **Inconclusive by timeout (refalsify spec §2.3, third verdict).** A probe
/// that exceeds the task's own `run_timeout_secs` is environmental, not
/// semantic: it injects and stamps `inconclusive`, and the episode STAYS
/// verified. The organ's law forbids the probe's infrastructure costing a
/// task its injection, and under v2 no probe outcome ever contradicts
/// (design §5, `organ_after_run`): a genuine clean nonzero exit means
/// `premise_held`, not a contradiction.
///
/// `d.txt` is the uncited-file trick again: `0` at mint (an instant exit 0
/// that clears the mint bar), `10` at retrieval, against a second task whose
/// `ExecBounds::run_timeout_secs` is 1.
#[test]
fn a_timed_out_probe_is_inconclusive_and_injects() {
    let dir = fresh_dir("refalsify", "timeout");
    let m = {
        let sb = dir.join("sandbox");
        std::fs::create_dir_all(&sb).unwrap();
        std::fs::write(sb.join("d.txt"), b"0").unwrap();
        mint(&dir, "sleep $(cat d.txt)", 1)
    };
    std::fs::write(m.sb.join("d.txt"), b"10").unwrap();

    let mut spec = spec_for(GOAL, &m.grant, &m.sb);
    spec.bounds.run_timeout_secs = 1;

    let p = probe(&m, &dir, spec, memory_ctx(&dir, true, true));
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert_eq!(
        p.stamp,
        (
            "injected".to_string(),
            Some(m.episode_id.clone()),
            1,
            Some("inconclusive".to_string())
        ),
    );
    assert_eq!(
        memory_prompts(&dir),
        1,
        "an inconclusive probe never costs the task its injection"
    );
    assert_eq!(
        p.stored,
        untouched(),
        "a timeout is not evidence the lesson is wrong"
    );
    assert!(p.contradicted.is_empty(), "{:?}", p.contradicted);
}

/// **Inconclusive by signal death (refalsify spec §2.3).** `exec_run`
/// reports a signal-killed child as `failed: false` with the pinned outcome
/// `"ran sh exit -1"` — `-1` being its "no exit code" sentinel, not a real
/// exit. Reading that as a clean nonzero exit would wrongly stamp
/// `premise_held` — claiming confirming evidence over a `SIGKILL` that
/// isn't real — instead of the honest `inconclusive`, so it must classify
/// `inconclusive`.
///
/// Driven through the full fixture rather than by calling the classifier
/// directly: the classifier is private to `task::registry`, and the property
/// that matters is that a real signal death reaches it as the sentinel and
/// leaves the store alone. `boom.txt` is the uncited marker — absent at mint
/// (so `exit 0` clears the bar), present at retrieval (so the shell kills
/// itself).
#[test]
fn classify_probe_calls_signal_death_inconclusive() {
    let dir = fresh_dir("refalsify", "signal");
    let m = mint(&dir, "[ -f boom.txt ] && kill -9 $$; exit 0", 1);
    std::fs::write(m.sb.join("boom.txt"), b"").unwrap();

    let p = probe(
        &m,
        &dir,
        spec_for(GOAL, &m.grant, &m.sb),
        memory_ctx(&dir, true, true),
    );
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert_eq!(
        p.stamp,
        (
            "injected".to_string(),
            Some(m.episode_id.clone()),
            1,
            Some("inconclusive".to_string())
        ),
        "a signal death is not a nonzero exit and must never be stamped premise_held"
    );
    assert_eq!(memory_prompts(&dir), 1);
    assert_eq!(
        p.stored,
        untouched(),
        "the episode STANDS — nothing measured it wrong"
    );
    assert!(p.contradicted.is_empty(), "{:?}", p.contradicted);
}

/// **Oversize outranks the probe (refalsify spec §2, as amended).** The
/// implemented order is retrieve → render → oversize gate → probe, and the
/// amendment states the behavioral consequence: *an episode the oversize rule
/// has already turned silent is never executed.* With `[memory] refalsify`
/// ON, a covering grant, and a stored command that would leave a trace, the
/// stamp is `("silent", None, refalsify: None)` and the canary stays gone.
///
/// The canary is what makes this a behavioral test rather than a restatement
/// of the stamp. `refalsify: None` alone is weak evidence — the oversize
/// return hardcodes it, so a probe hoisted above that gate would still stamp
/// `None` while having spent a subprocess. The file's absence is the only
/// observation that says *nothing ran*.
///
/// **Why the `Degraded` assertion is load-bearing, not decoration.** A
/// fingerprint miss stamps `("silent", None, 1, None)` too, and would leave
/// the canary equally absent — so without proof that the oversize branch is
/// the one that fired, this test could pass for a reason that has nothing to
/// do with the probe order. The degraded row naming the episode and the
/// injection bound is that proof.
///
/// The oversized episode is hand-minted straight into the store, mirroring
/// `memory_task_test.rs`'s `an_oversized_memory_block_is_skipped_and_stamped_silent`
/// and for its reason: the branch under test reads `render_memory_block`'s
/// output length and nothing else, so driving a >16 KiB patch through the
/// real executor would make the test slow and mostly about `exec_patch`.
/// Everything retrieval gates on is real — the goal hash, the canonical
/// cited path, the sha256 of the actual workspace bytes — and so is the
/// stored argv, which is the fixture's `["sh","-c",CANARY_SCRIPT]` verbatim.
#[test]
fn an_oversized_episode_is_never_probed_even_with_the_flag_on() {
    let dir = fresh_dir("refalsify", "oversize-flag-on");
    let sb = sandbox(&dir);
    let grant = grant_with(&sb, &[SH]);
    let (pager, agent_id) = build_pager(&dir, vec![done_turn()]);
    let pager = Arc::new(Mutex::new(pager));
    let registry = TaskRegistry::new();
    let journal_path = dir.join("tasks.jsonl");
    let ctx = memory_ctx(&dir, true, true);

    let cited_path = sb.join("a.py").display().to_string();
    let cited = vec![CitedFile {
        path: cited_path.clone(),
        fingerprint: Fingerprint::Sha256(sha256_hex_bytes(BEFORE)),
    }];
    let hash = goal_hash(GOAL);
    let argv: Vec<String> = vec![SH[0].into(), SH[1].into(), CANARY_SCRIPT.into()];
    let record = EpisodeRecord {
        episode_id: episode_id(&hash, &cited),
        goal_hash: hash,
        goal_text: GOAL.to_string(),
        cited_files: cited,
        // The one oversized field: a whole-file patch body well past the
        // 16 KiB injection bound.
        landed_patches: vec![StoredPatch {
            path: cited_path,
            codec: "whole_file".to_string(),
            body: format!("x = {}", "9".repeat(20_000)),
        }],
        run_evidence: RunEvidence {
            argv: argv.clone(),
            outcome: "ran sh exit 0".into(),
        },
        trajectory: vec!["read".into(), "patch".into(), "run".into(), "done".into()],
        minted_by_model: "m".into(),
        minted_by_envelope: "V1".into(),
        status: "verified".into(),
        contradicted_by: None,
        minted_at: 1,
    };
    let oversized_id = record.episode_id.clone();
    let rendered = render_memory_block(&record).len();
    assert!(
        rendered > MEMORY_BLOCK_MAX_BYTES,
        "the fixture must actually be oversized: {rendered} bytes"
    );
    // Nothing but the size may explain the skip: the grant covers the stored
    // argv exactly, so `skipped_ungranted` is off the table, and the task is
    // mutating, so the demotion boundary is too.
    assert!(
        grant.check_command(&argv).is_ok(),
        "the grant must cover the stored argv, or a skip proves nothing"
    );
    {
        let store = ctx
            .store
            .as_ref()
            .expect("an operational organ has a store");
        let mut store = store.lock().expect("the store mutex is healthy");
        store.mint(record, 64).unwrap();
    }
    assert!(
        !canary_exists(&sb),
        "the fixture must start with no canary — nothing has run it yet"
    );

    let (task_id, result) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(GOAL, &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );

    assert_eq!(result.status, TaskStatus::Done, "{result:?}");
    let events = replay(&journal_path).unwrap();
    assert_eq!(
        stamp_for(&events, &task_id),
        ("silent".to_string(), None, 1, None),
        "an oversize skip stamps no verdict, because nothing was probed"
    );
    assert_eq!(
        memory_prompts(&dir),
        0,
        "no prompt may carry a block the organ declined to inject"
    );
    let reasons = degraded_reasons(&events);
    assert!(
        reasons
            .iter()
            .any(|r| r.contains(&oversized_id) && r.contains("injection bound")),
        "the size skip must be the branch that fired, and it must name itself: {reasons:?}"
    );
    assert!(
        !canary_exists(&sb),
        "an episode the oversize rule already silenced is NEVER executed \
         (refalsify spec §2 amendment) — the canary would have reappeared"
    );
    assert_eq!(
        stored_status(&dir, &oversized_id),
        untouched(),
        "an unprobed episode is accused of nothing"
    );
    assert!(
        contradicted_ids(&events).is_empty(),
        "{:?}",
        contradicted_ids(&events)
    );
}
