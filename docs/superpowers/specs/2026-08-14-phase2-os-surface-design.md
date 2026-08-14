# bloomery Phase 2 — the OS surface

**Date:** 2026-08-14
**Status:** Draft for review (Phase 1 merged at `853d1e0`; G2 passed).
**Parent:** `2026-08-14-bloomery-design.md` (the umbrella; its §5 Phase 2
line and laws §3 govern). Gates G1 and G4 are already pinned in
`docs/gates.md` — this spec builds their instruments, it does not touch
their numbers.

## 1. What Phase 2 is

Phase 1 built the serving daemon that doesn't lie. Phase 2 makes it an
**agent runtime**: agents stop being bare inference sessions and become
budgeted, capability-scoped workers that mutate system state only
through a validated codec — with a tiny policy model advising the
scheduler once it has earned the right to (gate G1). Castle VTT becomes
consumer #1 of the `/v1` surface along the way, and its real traffic
produces the journal corpora G1's replay benchmark needs.

Phase 2 is decomposed into four sub-phases, each with its own
implementation plan, in dependency order:

- **2a — pager/substrate hardening** (prerequisites from CARRIED-DEBT)
- **2b — the edit-codec syscall ABI** (gate G4)
- **2c — capability grants** (enforced at 2b's applier)
- **2d — the policy plane** (gate G1; needs 2b/2c journals as corpus)

VTT integration is not a sub-phase; it is an acceptance activity that
starts as soon as 2a ships (the `/v1` surface it needs already exists).

## 2. Sub-phase 2a — hardening (prerequisites)

Ordered exactly as `docs/CARRIED-DEBT.md` records them:

1. **Weights enter the reservation budget.** `load_model` charges the
   model's `weights_bytes` (from GgufMeta) against the static VRAM
   budget; `unload_model` credits it. `free_for_planner` becomes
   `budget − Σ resident kv_bytes − Σ loaded weights_bytes`. The G2
   evidence doc's pressure arithmetic (§2) is the design note: after
   this lands, eviction pressure arises naturally and the
   unmeasured-VRAM bench mode stops being the only way to exercise the
   planner. The static-budget convention itself is unchanged (standing
   ruling).
2. **`model_digest` strengthened to a full-file sha256** (streamed).
   Boot-time cost on an 8 GB blob is seconds and paid once; this is the
   pinned precondition for images ever outliving a boot. Digest-tagged
   image filenames keep working (the digest just gets stronger).
3. **Journal schema additions**: `AgentRemoved { id, reason }` (closes
   the phantom-`AgentCreated` replay gap) and `TaskStep` (defined in
   2b). Additions only — existing variants and field names are frozen
   (the G2 bench reads them).
4. **Equal-priority time-sharing** (design decision, resolved here):
   the planner keeps its strictly-lower-priority eviction rule — but
   the pager adds a **round-robin tiebreak at the request layer**: when
   a request is refused solely because all residents are equal
   priority, the pager may evict the least-recently-used equal-priority
   *idle* resident iff the incoming agent has been waiting longer than
   a configured quantum (default 30 s, journal-recorded). This
   preserves the deterministic planner (law 8) — the LRU tiebreak is
   mechanism, computed from journal-visible last-use timestamps, not
   model advice. Refusal remains the answer within a quantum.

2a has no gate; its acceptance is the existing suite plus new tests,
and one live re-run of the G2 **warm** class under natural (measured
budget) pressure to confirm the pressure path change didn't shift the
number class (recorded as evidence, not a gate re-read — G2 stands).

## 3. Sub-phase 2b — the edit-codec syscall ABI (gate G4)

The umbrella's §4.5, made concrete. New daemon surface:

```
POST /agents/{id}/task   { goal, grants, budget_tokens, max_steps }
GET  /agents/{id}/task   → status/transcript
```

`task` runs the loop robigo proved and Phase 1 hosts: render prompt →
model emits ONE action in the codec → deterministic applier validates
and executes → observation appended → repeat until `done`, refusal, or
budget/step exhaustion. Every step journals `TaskStep { id, step, verb,
outcome, duration_ms }` plus the existing infer events.

**The verb set (v1, closed):**

| verb | payload | executes as |
|---|---|---|
| `read`  | path, optional line range | bounded file read within granted roots |
| `find`  | pattern, path prefix | bounded grep within granted roots |
| `patch` | path + search/replace or whole-file body (codec per profile) | atomic write-with-verify within granted write roots |
| `run`   | argv | subprocess from the granted command allowlist, bounded output + timeout |
| `done`  | summary | terminates the task |

**Codec rules (laws 3 and the black-oxide lessons, binding):**

- Text envelope, never grammar-forced. The applier validates and
  **re-asks** on violation, with a typed diagnostic naming both the
  defect and the expected shape (repair-loop ergonomics — the +10pp
  lives there). Max 2 re-asks per step, then the step fails honestly.
- Patch codec chosen **per model from its assay profile** (search/replace
  vs whole-file), exactly as the umbrella pins.
- `applies-and-parses` is the landing lens: a patch lands iff the codec
  applies and the result parses for the file's language (Python and
  plain-text checkers in v1; the lens is named in every record).

**Gate G4 (pinned):** per model, landing under this real envelope
≥80% or the model is demoted to a narrower verb set (read/find/done)
or refused for mutating roles. The G4 instrument is a fixture task set
run through the daemon's own task loop — measured at admission time
alongside the assay POST (a `codec` probe extension), not a one-off.

## 4. Sub-phase 2c — capability grants

Grants are explicit, task-scoped, and checked by the applier — never
ambient (umbrella §4.2). v1 grant model:

```json
"grants": {
  "read_roots":  ["/abs/path", ...],
  "write_roots": ["/abs/path", ...],
  "commands":    [["cargo", "test"], ["pytest"], ...],
  "network": false
}
```

- Path checks are canonical-path prefix checks (symlinks resolved
  before the check; a path escaping its root is a `GrantViolation`,
  journaled, step failed — the task continues, the model is told).
- `commands` are argv-prefix allowlists; `run` may append arguments but
  not change the prefix. Subprocesses inherit **no** network in v1
  (`network` is reserved, always false — refusing is honest).
- Worst-case successful prompt injection spends the task's own budget
  inside the task's own grants — the headline property. A red-team
  fixture set (injection attempts in file contents that try to widen
  scope) becomes part of the suite.
- The HTTP layer accepts grants only at task creation; nothing a model
  emits can modify grants (no verb exists for it).

## 5. Sub-phase 2d — the policy plane (gate G1)

- A **policy model** (0.5–3B, verdict-admitted like any model, its own
  assay profile) is consulted by the pager at scheduling decision
  points: eviction victim choice among planner-valid candidates, and
  the 2a time-sharing tiebreak. It advises; the deterministic rule
  executes when the advice is invalid, late (>500 ms), or
  contract-violating (law 8 — fail-closed, unchanged).
- Input: a compact state summary rendered from the journal's live
  state (residents, priorities, last-use, budgets) — never raw
  transcripts. Output: one action in a tiny closed codec (same
  envelope discipline as 2b).
- **Gate G1 (pinned):** on a frozen replay benchmark built from
  recorded journals (VTT traffic + task-loop corpora), policy must beat
  the deterministic heuristic by ≥10% useful-work-per-GPU-second at
  ≤5% contract violations and ≤500 ms decisions — else v1 ships
  deterministic-only and LLM policy is demoted to human-request
  granularity. The replay harness consumes journals only (pinned
  metric is journal-computable); building it is most of 2d.
- 2d starts last because the corpus must exist first.

## 6. VTT as consumer #1 (acceptance activity, starts after 2a)

Castle VTT's AI settings point at bloomery's `/v1` (base_url swap —
its OpenAI-compatible client already works). What VTT gains, mapped to
its 2026-08-12 audit findings: honest `prompt_too_large` errors instead
of silent front-truncation; real usage counts; a measured window
instead of a daemon-default `num_ctx`; per-request `max_tokens`
honored. Acceptance: the PDF-import extraction path run live against
bloomery with an oversized chunk producing a structured refusal (not a
confident-empty extraction), recorded as evidence. Any VTT-side fixes
belong to the VTT repo, not this one.

## 7. Non-goals (Phase 2)

- No bare metal, no appliance image (Phase 3), no semantic VFS (G3
  stays unattempted), no streaming-token SSE rewrite, no restart-
  survivable KV images (digest work in 2a is the precondition, not the
  feature), no network grants, no multi-GPU, no non-NVIDIA.

## 8. Risks

- **G4 may demote every 7B** (robigo's 1.06% looms). That is a valid
  outcome, not a failure: read/find/done agents with honest refusal
  are still useful, and the fine-tune flywheel (umbrella §7) is the
  recorded escalation path.
- **G1 may kill the policy plane** — pre-registered as shippable
  (deterministic-only v1).
- The applier executes model-chosen `run` commands; the grant model is
  the security boundary and gets the red-team fixtures before any
  default-config ships.
- Sub-phase creep: each sub-phase gets its own plan and lands
  independently; 2b is useful without 2c only in trusted-local use, so
  2b and 2c ship together or 2b stays behind a config flag
  (`tasks_enabled = false` default until 2c lands).

## 9. Deliverable order

1. Plan + execute **2a** (hardening) — small, unblocks everything.
2. VTT live acceptance (evidence doc in this repo, fixes in VTT's).
3. Plan + execute **2b + 2c together** (task ABI + grants, G4
   instrument included, `tasks_enabled` default-off until both land).
4. Corpus collection (journals from 2b/2c + VTT use).
5. Plan + execute **2d** (replay harness, policy model, G1 reading).
