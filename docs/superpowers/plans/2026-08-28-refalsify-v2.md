# Refalsify v2 — Premise-Verdict Probe Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the v1 probe's exit-0-injects/nonzero-contradicts verdicts with the premise model: clean nonzero → inject (`premise_held`), exit 0 → silent without store mutation (`premise_gone`); no probe ever contradicts.

**Architecture:** One seam in `crates/bloomery-daemon/src/task/registry.rs` (`organ_before_run`'s verdict handling + spelling remap after `classify_probe`), test flips + two new permanent pins in `crates/bloomery-daemon/tests/memory_refalsify_test.rs`, then a docs/records commit. Everything else — coverage pre-check, demotion, oversize gate, inconclusive arms, `classify_probe` parsing — is untouched.

**Tech Stack:** Rust (cargo), bloomery-daemon.

**Spec:** `docs/superpowers/specs/2026-08-28-refalsify-v2-class-aware-design.md` — the binding authority; where this plan and it disagree, the spec wins. Its §2 verdict table and §4 test list are the requirements.

## Global Constraints

- Work in a worktree branch off bloomery master `98ed3b5`; merges/pushes only on Brice's explicit rulings.
- Runner: `cargo test --workspace` (full output to a file + `echo exit=$?`; never pipe through tail/head without capturing exit); ALWAYS `cargo build -p bloomery-daemon --features vulkan` LAST after the final test run (clobber rule). `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` clean before commit.
- Mutation checks: apply → covering test must FAIL (quote the assert) → revert → `touch` the file → re-run green.
- The mint-bar invariant (every episode has landed patches — `memory/mint.rs::verifying_run` requires a successful patch step) is a comment-cited fact, never a code branch.
- Stamp spelling set after v2: `skipped_ungranted`, `inconclusive`, `premise_held`, `premise_gone`, absent. `passed`/`failed` retire from reachable verdicts; no parser/consumer change (they were `&'static str` pass-throughs).

---

### Task 1: v2 verdict mapping + test pins (TDD + mutation)

**Files:**
- Modify: `crates/bloomery-daemon/src/task/registry.rs` (verdict remap + the `Some("failed")` contradiction block ~lines 489–536; doc comments at the module header ~74–76, `classify_probe` ~330–345, and the probe block comments, updated to cite the v2 spec)
- Test: `crates/bloomery-daemon/tests/memory_refalsify_test.rs` (two tests rewritten, two added; file doc comment updated)

**Interfaces:**
- Consumes: `classify_probe(&Observation) -> &'static str` unchanged; fixture helpers `mint`, `probe`, `memory_ctx`, `spec_for`, `stamp_for`, `memory_prompts`, `stored_status`, `untouched`, constants `BEFORE`, `SH`, `CANARY_SCRIPT`, `CANARY`, `GOAL` — all existing in the test file.
- Produces: `OrganDecision.refalsify` now carries `"premise_held"`/`"premise_gone"` on the clean arms; Task 2's records cite this task's commit.

- [ ] **Step 1: Write the failing tests.** In `memory_refalsify_test.rs`:

(a) NEW permanent pin — the erratum's spike inverted (drift-free repeat, verification keyed to the cited file's goal state):

```rust
/// **The erratum pin (refalsify v2 spec §4).** A drift-free exact repeat of a
/// patch-class episode: the stored verification checks the CITED file's goal
/// state, and nothing changes after mint besides the fixture's own reset to
/// BEFORE — the match condition itself. v1 contradicted this true lesson
/// (2026-08-28 domain-of-validity erratum, demonstrated live); v2 reads the
/// failure as the premise holding and injects.
#[test]
fn a_drift_free_repeat_probes_premise_held_and_injects() {
    let dir = fresh_dir("premise-held");
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
```
(Adjust the stamp tuple's exact shape to `stamp_for`'s — read it; the four
asserted facts are mode `injected`, the episode id, `premise_held`, store
row untouched.)

(b) NEW premise_gone pin — stored verification passes on the pre-state
(`CANARY_SCRIPT` is exactly that), with the third-task re-probe:

```rust
/// **premise_gone (v2 spec §2/§4).** The stored verification passes on the
/// matched state: the premise is gone, the lesson is NOT false — silent, no
/// injection, no store mutation, and the next identical retrieval re-probes
/// (observed by the canary the command writes: deleted between tasks, it can
/// only reappear if a probe ran).
#[test]
fn a_passing_probe_is_premise_gone_silent_unmutated_and_reprobes() {
    let dir = fresh_dir("premise-gone");
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
        ("silent".to_string(), None, 1, Some("premise_gone".to_string())),
        "a satisfied premise is silence, not evidence against the lesson"
    );
    assert_eq!(memory_prompts(&dir), 0, "byte-identical to a stranger's prompt");
    assert_eq!(p.stored, untouched(), "premise_gone never touches the store");

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
        ("silent".to_string(), None, 1, Some("premise_gone".to_string())),
    );
    assert_eq!(contradicted_ids(&events).len(), 0, "no accusation, ever");
}
```

(c) REWRITE `a_passing_probe_injects_and_stamps_passed` — its behavior is
now (b)'s. Either delete it in favor of (b) (if (b) covers every
observation it made — check its asserts first) or reduce it to whatever
unique observation remains; do not keep a test asserting v1 semantics.
(d) REWRITE `a_failing_probe_contradicts_silences_and_stamps_failed` →
rename `an_uncited_drift_failure_reads_premise_held_and_injects`: keep its
fixture (mint `"exit $(cat flag.txt)"`, then drift the uncited `flag.txt`
to `1`), flip expectations to: stamp mode `injected` + `premise_held`,
`memory_prompts == 1`, store `untouched()`, and its third-task block
becomes: retrieval still matches, probe runs again, same stamp — with a
comment citing v2 spec §1's named limitation (an uncited-drift failure is
indistinguishable from premise-held without pre-state evidence; the
passive path owns the aftermath).
(e) UPDATE the file's module doc comment (lines ~1–20) to describe the v2
verdict model, citing the v2 spec path.

- [ ] **Step 2: Run the four touched tests — expect (a), (b) RED (v1 contradicts/injects respectively), (c)/(d) RED under their new expectations.** `cargo test -p bloomery-daemon --test memory_refalsify_test > /tmp/claude-refv2-red.txt 2>&1; echo exit=$?` — quote the failures in the report.

- [ ] **Step 3: Implement** in `registry.rs`. After the `Some(classify_probe(&obs))` line, remap the clean spellings (comment citing v2 spec §2 and the mint-bar invariant via `verifying_run`):

```rust
        // Refalsify v2 (spec 2026-08-28 §2): every mintable episode is
        // patch-class — memory/mint.rs::verifying_run requires a landed
        // patch — so the stored verification is a post-condition of the
        // fix and the matched state is the world BEFORE it. The clean
        // outcomes therefore invert: a failure CONFIRMS the premise
        // ("the defect is present") and injects; a pass means the world
        // no longer needs the lesson — silent, and never an accusation.
        let verdict = match verdict {
            Some("failed") => Some("premise_held"),
            Some("passed") => Some("premise_gone"),
            other => other,
        };
```
Replace the whole `if verdict == Some("failed") { ... mark_contradicted ... }` block with the premise_gone silent return (the contradiction machinery, its three-arm match, and its comments are deleted — `organ_after_run`'s passive path keeps `mark_contradicted`'s only remaining caller):

```rust
    if verdict == Some("premise_gone") {
        // v2 spec §2: the lesson is not false — no injection, no store
        // mutation, nothing journaled beyond the stamp; the next identical
        // retrieval re-probes.
        return OrganDecision {
            mode: "silent",
            candidates_checked: retrieval.candidates_checked,
            injected_id: None,
            block: None,
            refalsify: verdict,
        };
    }
```
Update the stale doc comments: module header (~74–76) and the probe block comment now cite the v2 spec; `classify_probe`'s doc keeps its parsing story but its "Only a genuine nonzero exit contradicts" sentence becomes "Only a genuine nonzero exit can hold the premise" with a pointer to the caller's v2 remap. Remove `agent_id` from `organ_before_run`'s signature ONLY if the contradiction deletion makes it unused (check; if unused, drop it at the call site too — dead params are surface).

- [ ] **Step 4: Run the test file → PASS**, then full suite: `cargo test --workspace > /tmp/claude-refv2-green.txt 2>&1; echo exit=$?` — expect 0 failures (the four rewritten/new tests green; every other refalsify test untouched and green).

- [ ] **Step 5: Mutation checks** (spec §4; each: mutate → named test FAILS, quoted → revert + touch → green): (1) swap the two remap arms (`premise_held` ↔ `premise_gone`) — killed by both pins; (2) delete the `premise_gone` early return (fall through to inject) — killed by (b)'s `memory_prompts == 0` + stamp asserts; (3) reintroduce a `mark_contradicted` call on the `premise_held` arm — killed by (a)/(d)'s `untouched()` store asserts.

- [ ] **Step 6: fmt + clippy clean; vulkan featured build LAST; commit:**

```bash
git add crates/bloomery-daemon/src/task/registry.rs crates/bloomery-daemon/tests/memory_refalsify_test.rs
git commit -m "feat: refalsify v2 — premise verdicts; the probe never contradicts (closes the 2026-08-28 erratum)"
```

### Task 2: Records — spec pointers, erratum closure, CARRIED-DEBT

**Files:**
- Modify: `docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md` (§2.3 and §6 dated pointers), `docs/CARRIED-DEBT.md`

**Interfaces:**
- Consumes: Task 1's commit sha (cite it).

- [ ] **Step 1:** In the v1 spec: append to the §2.3 amended pointer and the §6 erratum block one dated line each: `**Superseded 2026-08-28 by refalsify v2** (docs/superpowers/specs/2026-08-28-refalsify-v2-class-aware-design.md, commit <task-1-sha>): the clean-outcome verdicts invert to premise_held/premise_gone and no probe contradicts.` Append-only — no existing sentence is deleted or reworded.
- [ ] **Step 2:** `docs/CARRIED-DEBT.md`: append an amendment noting the erratum is closed by v2 (cite spec + commit), that `passed`/`failed` are retired-but-parsed spellings, and that the battery re-registration against v2 remains open.
- [ ] **Step 3:** Docs-only — do NOT run cargo (preserves the featured binary). Commit: `docs: v1 refalsify spec superseded-pointers + CARRIED-DEBT — erratum closed by v2`.
