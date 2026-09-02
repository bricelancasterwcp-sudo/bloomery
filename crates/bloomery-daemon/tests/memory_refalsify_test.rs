//! Refalsify: the premise verdicts that decide whether a retrieved episode
//! is still true before it is injected.
//!
//! The flag-off baseline, a drift-free repeat reading `premise_held`, a
//! passing probe reading `premise_gone` (silent, unmutated, re-probed), and
//! an uncited drift failure that still reads `premise_held`.
//!
//! **Split 2026-09-01** (carried-debt slice D): this file was 1238 lines.
//! The skip and inconclusive lanes are in `memory_refalsify_skip_test.rs`;
//! fixtures shared with `memory_task_test` are in `tests/common/memory.rs`,
//! and the ones specific to this pair in `tests/common/refalsify.rs`.

mod common;

use bloomery_core::journal::{replay, Event};
use bloomery_daemon::task::TaskStatus;
use common::memory::{
    contradicted_ids, fresh_dir, memory_ctx, memory_prompts, mint_ids, spec_for, BEFORE,
};

use common::refalsify::{
    canary_exists, drive, mint, probe, stamp_for, store_rows, untouched, CANARY, CANARY_SCRIPT,
    GOAL,
};

/// The `verb` of every `TaskStep` row on the journal, in append order.
///
/// `Event::TaskStep` carries an `AgentId` but no `task_id`, so "how many
/// steps did THIS task journal" is necessarily read as a delta across the
/// task under test. That is sound in this file and nowhere near a general
/// rule: every test here drives its tasks strictly one at a time (module
/// docs), so nothing else can be appending steps in the window.
fn task_step_verbs(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::TaskStep { verb, .. } => Some(verb.clone()),
            _ => None,
        })
        .collect()
}

/// **Flag-off identity (refalsify spec §5).** With `[memory] refalsify` off,
/// an exact repeat injects exactly as it did before this slice existed and
/// its stamp carries no verdict at all.
///
/// The canary is what makes this stronger than byte-diffing two prompts: the
/// episode's stored verification command WRITES a file, so a probe running
/// here — passing or not — would leave a trace. Its absence is a direct
/// observation that nothing was executed at the retrieval moment.
#[test]
fn flag_off_injects_without_probing_and_stamps_none() {
    let dir = fresh_dir("refalsify", "flag-off");
    let m = mint(&dir, CANARY_SCRIPT, 1);
    assert!(
        !canary_exists(&m.sb),
        "the fixture must start the second task with no canary"
    );

    let p = probe(
        &m,
        &dir,
        spec_for(GOAL, &m.grant, &m.sb),
        memory_ctx(&dir, true, false),
    );
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert_eq!(
        p.stamp,
        ("injected".to_string(), Some(m.episode_id.clone()), 1, None),
        "flag-off keeps today's behavior and stamps no verdict"
    );
    assert_eq!(
        memory_prompts(&dir),
        1,
        "the injected task's prompt must carry the rendered block — exactly \
         one prompt can, the probed task having exactly one turn and the \
         minting task having retrieved from an empty store"
    );
    assert!(
        !canary_exists(&m.sb),
        "no probe may run with the flag off — the canary would have reappeared"
    );
    assert_eq!(p.stored, untouched());
    assert!(p.contradicted.is_empty(), "{:?}", p.contradicted);
}

/// **The erratum pin (refalsify v2 spec §4).** A drift-free exact repeat of a
/// patch-class episode: the stored verification checks the CITED file's goal
/// state, and nothing changes after mint besides the fixture's own reset to
/// BEFORE — the match condition itself. v1 contradicted this true lesson
/// (2026-08-28 domain-of-validity erratum, demonstrated live); v2 reads the
/// failure as the premise holding and injects.
#[test]
fn a_drift_free_repeat_probes_premise_held_and_injects() {
    let dir = fresh_dir("refalsify", "premise-held");
    let m = mint(&dir, "grep -q 'x = 2' a.py", 1);
    assert_eq!(std::fs::read(m.sb.join("a.py")).unwrap(), BEFORE);

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
            Some("premise_held".to_string())
        ),
        "the failing probe confirms the matched premise and injects"
    );
    assert_eq!(memory_prompts(&dir), 1, "the lesson reached the prompt");
    assert_eq!(p.stored, untouched(), "no probe ever contradicts under v2");
}

/// **premise_gone (v2 spec §2/§4).** The stored verification passes on the
/// matched state: the premise is gone, the lesson is NOT false — silent, no
/// injection, no store mutation, and the next identical retrieval re-probes
/// (observed by the canary the command writes: deleted between tasks, it can
/// only reappear if a probe ran).
#[test]
fn a_passing_probe_is_premise_gone_silent_unmutated_and_reprobes() {
    let dir = fresh_dir("refalsify", "premise-gone");
    let m = mint(&dir, CANARY_SCRIPT, 2);

    let p = probe(
        &m,
        &dir,
        spec_for(GOAL, &m.grant, &m.sb),
        memory_ctx(&dir, true, true),
    );
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert!(canary_exists(&m.sb), "the probe really executed");
    assert_eq!(
        p.stamp,
        (
            "silent".to_string(),
            None,
            1,
            Some("premise_gone".to_string())
        ),
        "a satisfied premise is silence, not evidence against the lesson"
    );
    assert_eq!(
        memory_prompts(&dir),
        0,
        "byte-identical to a stranger's prompt"
    );
    assert_eq!(
        p.stored,
        untouched(),
        "premise_gone never touches the store"
    );

    // Third identical task: nothing was contradicted, so retrieval matches
    // again and the probe runs again — no memoized skip.
    let _ = std::fs::remove_file(m.sb.join(CANARY));
    let (next_id, next) = drive(
        &m.registry,
        &m.pager,
        &m.agent_id,
        spec_for(GOAL, &m.grant, &m.sb),
        &m.journal_path,
        Some(memory_ctx(&dir, true, true)),
    );
    assert_eq!(next.status, TaskStatus::Done, "{next:?}");
    assert!(canary_exists(&m.sb), "the second probe also ran");
    let events = replay(&m.journal_path).unwrap();
    assert_eq!(
        stamp_for(&events, &next_id),
        (
            "silent".to_string(),
            None,
            1,
            Some("premise_gone".to_string())
        ),
    );
    assert_eq!(contradicted_ids(&events).len(), 0, "no accusation, ever");
}

/// **No re-append, no phantom step (v2 spec §2, "no store mutation").** Two
/// properties of `premise_gone` the pin above does not observe, kept alive
/// from the retired `a_passing_probe_injects_and_stamps_passed` because
/// nothing else in this file pins them: the store FILE gains no row (not
/// even an identical re-mint, which append-only last-writer-wins semantics
/// could hide from an `episodes()` count — see [`store_rows`]'s own doc
/// comment), and the probe itself journals no `TaskStep` — the probed task's
/// steps are exactly its own one `done`, because the probe is not a model
/// action and never renders into the transcript.
#[test]
fn a_premise_gone_probe_appends_no_store_row_and_journals_no_step() {
    let dir = fresh_dir("refalsify", "premise-gone-untouched");
    let m = mint(&dir, CANARY_SCRIPT, 1);

    // The fixture's own baseline: one minting task, one minted row, its four
    // real steps. Both later assertions are deltas against these.
    let rows_after_mint = store_rows(&dir);
    assert_eq!(rows_after_mint, 1, "the mint appended exactly one row");
    let steps_after_mint = task_step_verbs(&replay(&m.journal_path).unwrap());
    assert_eq!(
        steps_after_mint,
        ["read", "patch", "run", "done"],
        "the minting task's own four steps are the baseline"
    );

    let p = probe(
        &m,
        &dir,
        spec_for(GOAL, &m.grant, &m.sb),
        memory_ctx(&dir, true, true),
    );
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert!(
        canary_exists(&m.sb),
        "the probe must have really executed the episode's stored command"
    );
    assert_eq!(
        p.store_rows, rows_after_mint,
        "a premise_gone probe appends nothing to the store (v2 spec §2): the \
         stamp is the durable evidence"
    );
    assert_eq!(
        task_step_verbs(&replay(&m.journal_path).unwrap()),
        ["read", "patch", "run", "done", "done"],
        "the probed task journals exactly one TaskStep — its own `done` — and \
         the probe, which ran a real subprocess, journals none"
    );
}

/// **An uncited-drift failure reads premise_held (v2 spec §1's named
/// limitation).** `flag.txt` holds `0` when the episode is minted and `1`
/// when it is retrieved; the model never reads `flag.txt`, so it is not in
/// `cited_files` and the exact gate is honestly satisfied. Under v1 this was
/// the slice's whole point — early detection of stale-but-uncited state.
/// Under v2 a verification that is state-independent of what the patches
/// actually touched is indistinguishable from a genuinely held premise
/// without recorded pre-state evidence (out of scope, not foreclosed): the
/// stored command fails, so the probe reads `premise_held` and injects. The
/// injection is noise, not damage — if the lesson really is stale, the
/// pre-existing PASSIVE path (`organ_after_run`: this probed task received
/// the injection and then landed no run of its own to re-verify it) owns
/// the aftermath, exactly as it would have before refalsify existed.
#[test]
fn an_uncited_drift_failure_reads_premise_held_and_injects() {
    let dir = fresh_dir("refalsify", "uncited-drift");
    let m = {
        // `flag.txt` must hold "0" before the minting run, or the mint bar
        // (exit 0 after the last landed patch) is never cleared.
        let sb = dir.join("sandbox");
        std::fs::create_dir_all(&sb).unwrap();
        std::fs::write(sb.join("flag.txt"), b"0").unwrap();
        mint(&dir, "exit $(cat flag.txt)", 2)
    };

    // The one byte that turns the stored verification stale — in a file no
    // citation covers, so retrieval still matches exactly.
    std::fs::write(m.sb.join("flag.txt"), b"1").unwrap();

    let p = probe(
        &m,
        &dir,
        spec_for(GOAL, &m.grant, &m.sb),
        memory_ctx(&dir, true, true),
    );
    let probed_id = p.task_id.clone();
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert_eq!(
        p.stamp,
        (
            "injected".to_string(),
            Some(m.episode_id.clone()),
            1,
            Some("premise_held".to_string())
        ),
        "an uncited-drift failure is indistinguishable from a held premise \
         under v2 — it injects"
    );
    assert_eq!(memory_prompts(&dir), 1, "the lesson reached the prompt");
    assert_eq!(p.stored, untouched(), "no probe ever contradicts under v2");

    // A third identical task: the bytes never drifted again (still `1`), and
    // the fingerprint gate never covered `flag.txt` in the first place, so
    // whether retrieval still matches this third time depends only on
    // whether anything contradicted the episode in between — and the
    // PASSIVE path (not the probe) is exactly the mechanism that can.
    assert_eq!(std::fs::read(m.sb.join("a.py")).unwrap(), BEFORE);
    let (next_id, next) = drive(
        &m.registry,
        &m.pager,
        &m.agent_id,
        spec_for(GOAL, &m.grant, &m.sb),
        &m.journal_path,
        Some(memory_ctx(&dir, true, true)),
    );
    assert_eq!(next.status, TaskStatus::Done, "{next:?}");
    let events = replay(&m.journal_path).unwrap();
    assert_eq!(
        stamp_for(&events, &next_id),
        ("silent".to_string(), None, 1, None),
        "the passive path contradicted the injected-but-unverified probe \
         task, so this repeat retrieves silence — v2 never memoizes a \
         probe's own verdict, but it never disarms the pre-existing passive \
         path either"
    );
    assert_eq!(
        contradicted_ids(&events),
        vec![(probed_id.clone(), m.episode_id.clone())],
        "one accusation, citing the probed task itself — the passive path's, \
         not the probe's: {events:?}"
    );
    assert_eq!(mint_ids(&events).len(), 1, "no follow-up task minted");
}
