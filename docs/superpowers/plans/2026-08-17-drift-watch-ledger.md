<!-- The drift-watch wave's process record (SDD ledger), copied verbatim at wave close. -->

# SDD ledger — plan: docs/superpowers/plans/2026-08-17-drift-watch.md

Spec: docs/superpowers/specs/2026-08-17-drift-watch-design.md (binding)
Worktree: ~/workspace/bloomery/.worktrees/drift branch feat/drift-watch, base ce744af, baseline 446 tests green (cargo test -p bloomery-core -p bloomery-daemon)

## Pre-flight conflict scan
| Tasks | Produces vs consumes | Finding |
|---|---|---|
| 1→3 | InstrumentPrecheck | consistent |
| 2→3/4/5 | ProfileStore/Blessed | consistent |
| 3→4 | GateOutcome → DriftStatus fold | consistent |
| 4↔post tests | "existing post tests unmodified" vs new Drift journal rows | post_test.rs (32 fns) shows no exact-sequence indexing on events; low risk. Escape hatch in T4 dispatch: an exact-full-sequence assertion found = report for ruling, never silently edit |
| 2 slug claim | plan says "reuse POST's slug rule" | **FINDING: no slug exists** — POST writes profiles_dir.join("{model}.json") raw (post.rs:~370). Ruling below |
| 6 PYTHONPATH | old pin at session scratchpad may be gone | T6 preflight re-derives how assay resolves on this box; noted, not blocking |

Ruling: plan Task 2's slug-reuse clause corrected — there is no slug helper; ProfileStore adopts POST's existing raw-name convention: current = "{model}.json" (POST's own file), previous = "{model}.previous.json", baseline = "{model}.baseline.json", transients = "{model}.transient-{sha8}.json". A '/' in a model key would already break POST's paths today — pre-existing constraint, ledgered as debt, NOT fixed in this wave. Cost if wrong: filename collisions for exotic keys — same exposure POST already has.

## Progress
Task 1: dispatched (opus implementer), BASE ce744af
Task 1: implementer DONE (commit 91bef04, 452 green; 13 hand-built fixtures gained honest probe_version values; instrument_id private — later task promotes)
Task 1: review Needs fixes — 1 Important (Comparable branch tested only on byte-identical documents; byte-equality impl survives the suite) -> fix round 1
Task 1: minor (deferred): fixtures live in daemon crate while parser is core (cross-crate include_str — settle before more consumers); fixture consts + provenance prose duplicated in 2 files; without_probe_version line-oriented trap latent on compact json; helper doc generality overstated; InstrumentPrecheck could derive Eq + wants Display for Task 3's journal row
Task 1: fix round 1/5 (1 addressed, 0 open; mutant empirically confirmed pre-fix then killed; third verbatim fixture sha-verified by re-reviewer)
Task 1: complete (commits ce744af..0e3f209, review clean after 1 fix round; 455 green)
Task 2: dispatched (opus implementer), BASE 0e3f209
Task 2: implementer DONE (commit 63f5f17, 466 green; rotate parses current itself + KeptUnparseable; full-sha claim / sha8 filename; shared profile_file_name pinned behaviorally)
Task 2: review Needs fixes — 2 Important (non-UTF-8 current escapes KeptUnparseable via read_to_string io::Error; mtime tiebreak load-bearing but unmutated/untested — equal-mtime branch never executes) -> fix round 1
Ruling (forward, for Task 3 dispatch): Drift journal rows carry reference_sha + current_sha (full 64-hex of file bytes at comparison time) beside the paths — identity claims like Blessed's sha, NOT measurement numbers, so spec §4's no-transcribed-numbers law stands; makes drift-step rows byte-verifiable (reviewer's ⚠: previous/current carry no digest claim anywhere otherwise).
Task 2: minor (deferred): retain_transient can prune the file it just retained (mtime preserved by rename — incidental invariant); partial prune drops accumulated dropped-record via ?; orphan .tmp never swept; rotate/retain return bare io::Result vs DriftError's own context argument; provenance String two-value set unenforced (pub const pair suggestion)
Task 2: fix round 1/5 (2 addressed, 0 open; UTF-8 class named, tiebreak pinned w/ honestly-scoped tmpfs limitation)
Task 2: complete (commits 0e3f209..a3bcb8c, review clean after 1 fix round; 468 green)
Task 2: minor (deferred, from re-review): tie test coupled to profile_doc hash ordering (latent — comment if template changes)
Task 3: dispatched (opus implementer), BASE a3bcb8c
Task 3: implementer DONE (commit 01804f6, 488 green; GateReading deviation = single-read identity; run_bounded shared pub(crate); undocumented exit carries Some(N))
Task 3: review Approved w/ 1 Important (no same-model check — crossed pair spawns and journals one model's row over another's document; happy-path tests normalize a cross-model pair) -> fix round 1
Task 3: minor (deferred): if-let on InstrumentPrecheck falls through to spawn (exhaustive match costs one line); timeout() reads the field not the spawn's cap (close via shared local); compare doc overclaims "before anything else could touch"; NotComparable literal repetition; diff_argv indirection style
Task 3: fix round 1/5 (1 addressed, 0 open; crossed-model refusal + dry-run fixture from assay git history, sha-verified; 490 green)
Task 3: complete (commits a3bcb8c..3c15f4b, review clean after 1 fix round)
Task 4: dispatched (opus implementer), BASE 3c15f4b
Task 4: implementer DONE (commit d33494f, 501 green; run_post_with_gate injection keeps signature; per-comparison confirms = both-drift boot costs 2 probes, named+rejected alternative in-code)
Task 4: review Needs fixes — 3 Important (failed confirm probe journals NOTHING — 600s dead probe zero durable trace, and a test pins the gap; confirm verdicts journal as raw re-diff words — a Transient finding reads "within-noise" like a clean pass; production run_post wiring unguarded — revert-to-pre-drift mutant survives suite) -> fix round 1
Ruling: confirm row's outcome string spells the SETTLED verdict ("confirmed"/"transient"/"unconfirmed: <named>") — one row per confirm as sanctioned, right name, no third row.
Task 4: minor (deferred): failed-bless + retention-failure Degraded branches untested; journal.rs:172 "two rows per boot" doc stale; Infra folded into Unmeasured needs string-sniffing to separate (enforcement slice wants them apart); unbounded assay stderr onto /status; boot-window cost note (confirms extend provisional-admission window worst-case N x probe_timeout) -> carry to Task 6 evidence doc as operator note
Task 4: fix round 1/5 (3 addressed, 0 open; confirm verdicts durable under their right names; production wiring pinned python-free)
Task 4: complete (commits 3c15f4b..e369c4a, review clean after 1 fix round; 501 green)
Task 5: dispatched (opus implementer), BASE e369c4a
Task 5: implementer DONE (commit 923b68f, 507 green; re-bless = provenance fold 'operator (replaced <sha>)', no schema change; Pager::bless_baseline + main.rs profiles_dir plumbing)
Task 5: review Needs fixes — 1 Important (provenance closed-set claim struck in drift/watch.rs but still FALSE in core journal.rs:156-160 schema doc + journal_blessed's own doc — the authoritative places; two doc edits) -> fix round 1
Task 5: minor (deferred): BlessError::Journal machine-indistinguishable from nothing-happened (baseline-replaced-but-unrecorded deserves its own code); bless route races auto_bless during POST window (mild: double Blessed rows w/ different provenance — provenance ambiguity in the live window; NOTE for Task 6 evidence doc); unreadable-old-baseline free text in the digest slot (prefix-distinguishable shape better); bless handler let-e-match departs file idiom (map_bless_error extraction)
Task 5: fix round 1/5 (1 addressed, 0 open; doc-only, both authoritative sites now state the prefix-family contract; plan artifact deliberately untouched — sound)
Task 5: complete (commits e369c4a..254ddb9, review clean after 1 fix round; 507 green)
Task 6: dispatched (opus implementer), BASE 254ddb9 — GPU idle 774MiB confirmed pre-dispatch; assay 0.9.0 via PYTHONPATH=~/workspace/assay/src; model = flywheel2 14B Q4
Task 6: implementer DONE (commit 23aa166; boots 1-3 all read spec-pinned outcomes first try; GPU clean 776MiB)
FINDING (assay-side, carry to assay debt/v1.8): assay 0.9.0 `diff --gate` on a v8-vs-v4 pair exits 0 "no drift beyond noise" while FIVE families vanish (long_output, tool_calling, 3 json cells) — literally true under its rules, consumer-dangerous; bloomery's §3 precheck is the only guard. ALSO: diff prose falsely reports dropped verdict.long_context for equal objects (prose bug, bloomery never parses prose); assay 0.5.0 has no diff subcommand (argparse exit 2 → not-comparable — unreachable behind precheck).
Task 6: review Approved w/ 1 Important (three verbatim/every claims are undisclosed subsets — journal rows omit ModelLoaded/AgentRemoved, config comments stripped, structural diff 4-of-6 paths) -> fix round 1
Ruling: minor 3 GRADUATES (the 839MiB final reading + compositor-drift explanation belongs in the COMMITTED doc — the report isn't durable); minor 4's two spec-text divergences get dated NON-SILENT FOOTNOTES in the spec this round (§5 content-addressing satisfied by journal sha fields not filename prefixes; §2 auto-bless spelling settled as "auto-first-profile" at 254ddb9) — controller-owned doc, house amendment pattern.
Task 6: minor (deferred): boot-3 config header comment divergence (same class as the Important, covered by its fix); "same shape as boot 2's" undersells the 34-vs-27 entry difference
Task 6: fix round 1/5 (all addressed; census arithmetic verified against on-disk journal 558 rows exactly; beyond-scope edit judged necessary-truth)
Task 6: complete (commits 254ddb9..b90f0cc, review clean after 1 fix round) — ALL 6 TASKS COMPLETE
PLAN GAP (controller): no CARRIED-DEBT slice-append task was planned; bloomery house pattern appends at merge — will dispatch with/after final-review fixes.
FINAL REVIEW: dispatching whole-branch (fable) on ce744af..b90f0cc
FINAL REVIEW: Ready-after-fixes (fable, 7 passes; suite/fmt/clippy independently re-run; fixture + live-baseline shas re-hashed; no seam drift; no contested rulings). Fix wave: (1) journal.rs Event::Drift schema doc false on two counts (three rows possible per model+boot; outcome spellings omit confirmed/transient/unconfirmed — the Blessed-class falsity, half-noted as a T4 minor, graduates); (2) CARRIED-DEBT slice append per the enumerated contents (settled rulings / deferred by task / assay-side carry / process lessons incl. the plan-template gap).
FINAL FIX WAVE round 1: Fix 1 ADDRESSED (one prose overclaim residual: retention-failure Degraded doesn't name the comparison); Fix 2 NOT ADDRESSED — four ledgered still-open items silently dropped (T1 helper-doc-generality; T2 provenance-const pair; T3 diff_argv style; T5 map_bless_error idiom) with counts matching only the truncated lists; plus one untraceable confirm-staging sweep claim. Ruling: this is incomplete execution of the enumerated fix, not a new finding — round 2 of the SAME wave (task-loop machinery, cap 5); no new scope admitted.
FINAL FIX WAVE round 2: all items addressed (re-reviewer re-verified counts against the ledger — T2 actually 6 covered; staging-sweep reasoning code-backed; Degraded split accurate). BRANCH DONE at b71da8d.
