# Plan — flywheel turn 6 (the honesty instrument)

Spec: `docs/superpowers/specs/2026-08-23-flywheel6-honesty-design.md` —
approved 2026-08-23 with rulings in its header; binding. Execution begun
2026-08-29 under Brice's delegation ("we will do as you recommend" —
turn 6 as the next arc); the spec's per-step Brice gates (§8.1's
pre-registration acknowledgment; §5.4's per-boot go) are read as covered
by that delegation, recorded in the ledger, with the standing
GPU-hygiene-check-before-boot discipline and a held GPU a STOP. Turn 6
is instrument-only, local, $0 (spec §1/§7) — the RunPod balance is a
turn-7 concern.

**Compatibility check at plan time (everything landed since 2026-08-23):**
memory organ + refalsify (v2, default-on) touch only the TASK path — the
codec probe never renders a memory block, and baseline boot configs carry
no `[memory]` table (default `enabled = false`), so every frozen
instrument still runs memory-off; window ladder is `TaskSpec`-gated,
default off; R9/v3 vendoring changed geometry derivation only. The
v1–v4 anti-drift byte-identity pins have stayed green through every
merge since, so the spec's §3.1 baseline ("v4 renders byte-identical to
today") still holds. No spec amendment needed.

## Phase A — branch `turn6-claim-audit` (spec §2, PR A)

The seven v4 journal pairs (paths + sha256 recorded in the audit doc's
pre-registration):

1. `2026-08-21-flywheel4-g5-{journal,tasks}.jsonl`
2. `2026-08-21-g5v4-stock14b-{journal,tasks}.jsonl`
3. `2026-08-21-g5v4-flywheel3-{journal,tasks}.jsonl`
4. `2026-08-22-g5v4-reap48-boot1-{journal,tasks}.jsonl`
5. `2026-08-22-g5v4-reap48-boot2-{journal,tasks}.jsonl`
6. `2026-08-23-flywheel5-boot1-{journal,tasks}.jsonl`
7. `2026-08-23-flywheel5-boot2-{journal,tasks}.jsonl`

Tasks:
- A1: audit doc pre-registration committed FIRST — spec §2.2's patterns
  verbatim, eligible rows, the seven paths + shas, the calibration rows
  named in advance (spec §2.3.4's five hand-read rows).
- A2 (TDD): `claim_audit` in `tools/evidence/endpoints.py` + CLI wiring
  in `recompute.py` (new JSON key; join reuse; exit 2 unchanged);
  `tests/test_claim_audit.py` freezes the exact pattern strings and
  covers every category with a match-and-nonmatch pair (sentence
  splitting; negation-position guard both directions; the "to is not a
  token" row; denial patterns; undeclared). Mutation checks: drop the
  negation guard → caught; bare-infinitive included → caught.
- A3: run over the seven journals; append the per-journal and
  calibration tables to the audit doc (tool JSON quoted verbatim; no
  tuning after running — a miss is written down); §5/§6 discipline
  sections.
- A4: evidence review (verifier re-derivation of every quoted number
  from the committed JSON) → fix wave → merge to master.

## Phase B — branch `turn6-envelope-v5` (spec §3–§5, PR B)

- B1 (Rust, TDD): `EnvelopeLens::V5` + `done_declares()`;
  `DoneCard::Declared` in both card branches; `validate_done` optional
  attributes + `evidence:` collection; `action_args(Done)`;
  flywheel-tool V5 passthrough. Anti-drift: v1–v4 prompt/card bytes
  pinned unchanged (existing snapshots must stay green untouched).
- B2 (factory): `done_v5` assembler; template ground-truth exposure;
  `gate_vocabulary` fifth set; `test_contamination_g5_v5`.
- B3: author `codec-tasks-v5-mixed` (16+16, composition = v4; new gate
  seed recorded; fresh vocabulary, disjointness asserted; `family` on
  every refuse row; `refusal_reason` = full ideal v5 done; freeze-time
  verbatim-evidence test) — reviewed before freeze, then frozen.
- B4: protocol doc + `gates.md` amendment committed BEFORE any v5 boot.
- B5 (TDD): the three declaration endpoints in `tools/evidence`
  (`outcome_consistent`, `evidence_grounded` incl. post-reference bytes
  for landed patches + `misaligned` kept apart, `reason_matches_family`
  from the `family` key only); synthetic category tests
  mutation-guarded per spec §5.3.
- B6: final whole-branch review → merge. Rust discipline: workspace
  suite, clippy, THEN featured vulkan build last.

## Phase C — baselines (spec §5.4)

Eight serial boots (4 models × 2, boot 1 the anchor, declared per model
before its first boot), GPU-hygiene check before each; G4 + G5-v5 +
declaration endpoints per boot; digest↔artifact sha; byte-identity
across boots reported; 24 artifacts + baselines doc; recompute tests
pinned to the committed journals; evidence review → fix wave → merge;
README + CARRIED-DEBT turn-6 append.

Artifact existence is re-verified at Phase C start (the four rows of
spec §5.4's table; the 14B-line geometry from the retained
`target/fw4-live/*` configs).
