# bloomery Phase 2b + 2c — the task ABI and capability grants

**Date:** 2026-08-14
**Status:** Draft for review (2a merged at `04090fc`).
**Parent:** `2026-08-14-phase2-os-surface-design.md` §3 (2b) + §4 (2c). The
umbrella laws §3 and gate G4 (pinned in `docs/gates.md`) govern. 2b and
2c ship together per the umbrella's §9 decision — 2b is useful without
2c only in trusted-local use, so the whole surface stays behind
`tasks_enabled = false` (default) until both land.

## 1. What this builds

Phase 1/2a made bloomery a serving daemon with budgeted, paged agents.
2b/2c turns an agent into a **worker**: given a goal and an explicit
grant, it runs the propose→validate→execute loop robigo proved — the
model emits one structured action, a deterministic Rust applier
validates and executes it against the agent's grants, the observation
is appended, and it repeats until `done`, refusal, or exhaustion. Every
action is capability-checked and journaled. This is the umbrella's
§4.5 syscall ABI (2b) enforced by §4.2 capabilities (2c).

Gate **G4** decides which models earn mutating verbs: per model,
edit-codec landing under the real task envelope must be ≥80% or the
model is demoted to read-only verbs. G4's instrument is built here and
measured at admission (a codec probe extending the assay POST).

Non-goals unchanged from the umbrella §7: no policy plane (2d), no
network grants, no restart-survivable state, no streaming rewrite.

## 2. Sub-phase decomposition (each its own implementation plan)

2b/2c is large; it decomposes into four plans, in dependency order:

- **P1 — the action codec + applier** (pure `bloomery-core`, GPU-free):
  parse a model turn into one typed `Action`; the deterministic
  appliers for each verb's *validation* (not yet execution); typed
  diagnostics for re-ask.
- **P2 — capability grants** (`bloomery-core` + daemon): the `Grant`
  type, canonical-path and argv-prefix checks, `GrantViolation`. Pure
  and unit-testable before any I/O.
- **P3 — the task loop + executors + HTTP surface** (daemon): wire the
  codec, grants, and real executors (bounded file read, grep,
  atomic patch-with-verify, sandboxed subprocess) into
  `POST /agents/{id}/task`, journaling `TaskStep`. `tasks_enabled` gate.
- **P4 — G4 codec probe + landing measurement** (daemon + assay
  integration): the fixture task set run through the real loop; the
  applies-and-parses landing lens; per-model verdict at admission;
  demotion. Live-measured, its own evidence doc.

VTT acceptance (umbrella §6) is not blocked by 2b/2c and proceeds in
parallel against the existing `/v1`.

## 3. The action codec (P1)

A model turn is text. The codec parses exactly one action from it,
envelope-constrained, never grammar-forced (laws 3). The envelope is a
fenced block the applier recognizes; everything outside it is ignored
prose (models narrate — that's fine).

```
<action verb="patch" path="src/foo.py">
<<<<<<< SEARCH
old line
=======
new line
>>>>>>> REPLACE
</action>
```

The verb set (v1, closed):

| verb | required attrs / body | validated shape |
|---|---|---|
| `read`  | `path`, optional `lines="A-B"` | path parses; range is `A ≤ B`, both ≥ 1 |
| `find`  | `pattern`, `path` (prefix) | pattern is a valid regex; non-empty |
| `patch` | `path` + a body in the model's profile-selected codec (search/replace or whole-file) | the codec parses; search/replace has both halves |
| `run`   | `argv` (JSON array in body) | non-empty argv; every element a string |
| `done`  | `summary` body | non-empty summary |

Codec rules (binding):

- **Exactly one action per turn.** Zero actions → diagnostic "no action
  found"; two+ → diagnostic "one action per turn, found N" (the applier
  does not guess which). Re-ask on either.
- **Typed diagnostics designed for the repair loop.** Every parse/shape
  failure returns an `ActionError` variant naming the defect *and* the
  expected shape (black-oxide's measured lesson: repair ergonomics
  dominate). E.g. `PatchNoSearchMarker { expected: "<<<<<<< SEARCH" }`.
- **Patch codec is per-model, from the assay profile** (search/replace
  vs whole-file), chosen at admission exactly as the umbrella pins. P1
  parses both; which one a given model is offered is P4's verdict.
- **Landing lens = applies-and-parses**: a patch *lands* iff the codec
  applies to the current file bytes AND the result parses for the
  file's language. P1 ships Python and plain-text parsers (Python via a
  `py_compile`-style syntax check subprocess in P3; in P1 the lens is a
  trait with a plain-text impl and a named `Unparsed` reason for
  unknown languages — never a false "lands"). The lens is named in
  every landing record (assay discipline).

`Action`, `ActionError`, the parser, and the validators are pure and
GPU-free — the whole of P1 is `cargo test` without a daemon.

## 4. Capability grants (P2)

Grants are explicit, task-scoped, checked by the applier, never
ambient (umbrella §4.2). The wire shape:

```json
"grants": {
  "read_roots":  ["/abs/a", "/abs/b"],
  "write_roots": ["/abs/b/out"],
  "commands":    [["cargo","test"], ["python","-m","pytest"]],
  "network": false
}
```

Enforcement (binding):

- **Path checks are canonical-prefix checks.** The applier resolves the
  action's path to a canonical absolute path (symlinks followed, `..`
  collapsed) *before* the check; a path that escapes every listed root
  → `GrantViolation`, journaled, the step fails, the task continues and
  the model is told. `read` checks `read_roots`; `patch` checks
  `write_roots` (a write root is not implicitly a read root — grant
  both if the model must read what it patches).
- **`commands` are argv-prefix allowlists.** `run`'s argv must start
  with a listed prefix exactly (element-wise); it may append arguments
  but never change or reorder the prefix. No shell — argv is exec'd
  directly, never through `sh -c`.
- **`network: false` is the only accepted value in v1** (reserved;
  refusing is honest). Subprocesses inherit a scrubbed environment with
  no proxy vars and are not given network namespaces bloomery can't
  guarantee — v1 documents that it does not sandbox the network itself
  and relies on `commands` being non-networking; a red-team fixture
  asserts a `run` of a curl-shaped command is only possible if the
  operator granted it (i.e. the boundary is the grant, and that's
  stated, not overclaimed).
- **Grants are immutable for the task's life.** They are accepted only
  at `POST /agents/{id}/task`; no verb can read or modify them, and the
  applier holds them by value. The worst case a successful prompt
  injection achieves is spending the task's own budget inside its own
  grants — the headline property, asserted by a red-team fixture set
  (file contents that try to talk the model into widening scope; the
  check is structural, so the model *cannot* comply even if convinced).

The `Grant` type and its check functions are pure `bloomery-core`
(canonicalization is `std::fs::canonicalize`, testable against tempdir
symlinks); the executors that consume a validated grant live in the
daemon (P3).

## 5. The task loop, executors, HTTP surface (P3)

```
POST /agents/{id}/task  { goal, grants, budget_tokens, max_steps }
                        → 202 { task_id }   (runs; poll for result)
GET  /agents/{id}/task  → { status, steps: [...], result }
```

Loop (binding shape, robigo-proven):

1. Render the prompt: goal + the verb card + accumulated observations
   (windowed to fit the agent's measured window — the 2a degradation
   discipline applies; refuse-with-arithmetic if even the goal + card
   won't fit).
2. Infer one turn (the existing pager `infer`, budget-charged).
3. Parse one action (P1). On `ActionError`: journal a `TaskStep` with
   `outcome` = the diagnostic, feed the diagnostic back, re-ask. **Max
   2 re-asks per step; then the step fails honestly** (journaled) and
   the loop continues to the next step (a stuck step is not a stuck
   task) — or ends if re-asks exhaust the step budget.
4. Grant-check the action (P2). Violation → `GrantViolation` outcome,
   step fails, continue.
5. Execute (the verb's executor). `TaskStep { id, step, verb, outcome,
   duration_ms }` journaled with the real outcome.
6. Repeat until `done`, `max_steps`, budget exhaustion, or a hard
   error. Terminal state and full transcript are the `GET` result.

Executors (binding bounds):

- `read`: bounded file read within `read_roots` (max bytes cap,
  configurable, default 256 KiB; over-cap → truncated-with-notice, not
  silent).
- `find`: the `find` executor shells nothing — it walks `read_roots`
  and matches the compiled regex, bounded result count (default 100,
  over → capped-with-notice).
- `patch`: **atomic write-with-verify** — apply the codec to current
  bytes in memory, run the landing lens (applies-and-parses), and only
  on a clean landing write to a temp file + rename; a failed landing is
  a step failure with the lens's reason, the file untouched (robigo's
  patch safety).
- `run`: `std::process::Command` with the exact argv (no shell),
  scrubbed env, a wall-clock timeout (default 120 s, the 2a
  poll-and-kill+reap pattern from `post.rs`), bounded captured output
  (stdout+stderr cap, default 64 KiB, over → truncated-with-notice),
  cwd defaulting to the first `write_root` (or first `read_root`).
  Exit code + captured output are the observation.

`tasks_enabled` (config, default `false`): when false, `POST
/agents/{id}/task` → 501 `{"error":"tasks_disabled"}`. The whole P3/P4
surface is dark by default until the operator opts in — the security
posture the umbrella requires.

Concurrency: tasks run under the same coarse pager `Mutex` as
everything else in Phase 1/2 (one GPU, serialized). A task holds the
lock only across each `infer` + apply, not across a `run` subprocess's
wall-clock (the subprocess executes outside the lock, like the assay
POST) — otherwise one slow test wedges the daemon.

## 6. G4 — codec landing measurement (P4)

- A frozen fixture task set: N single-defect repair tasks (robigo's
  shape — a failing check, a known one-line fix) across the Python and
  plain-text lenses, each with a verified reference landing. The set
  name travels in every record (assay's fixture-provenance discipline —
  the same lesson that produced assay's `codec-fixtures-v2`).
- The G4 probe runs each fixture through the **real** task loop against
  a candidate model and scores applies-and-parses landing. It extends
  the assay POST (a new `codec` probe family) so landing is measured at
  admission, per model, on this box's serving path.
- **Gate G4 (pinned, `docs/gates.md`):** landing ≥80% → the model keeps
  mutating verbs (`patch`, `run`); <80% → demoted to read-only
  (`read`, `find`, `done`) and the demotion is journaled and surfaced
  in `/status`. A demoted model can still run non-mutating tasks
  honestly.
- Wilson interval + provisional-verdict discipline (assay v1.3): a
  small-N landing rate carries its interval and is marked provisional
  when the interval straddles 80%; the profile says so rather than
  point-estimating.
- P4 is live-measured on this box and ships its own evidence doc, with
  the honest possibility (umbrella §8) that **every local 7B is
  demoted** — a valid outcome, not a failure; read-only agents with
  honest refusal are still useful, and the fine-tune flywheel is the
  recorded escalation.

## 7. Testing posture

- P1, P2: entirely pure/GPU-free (`cargo test --workspace`), including
  the red-team grant fixtures (a directory of adversarial file contents
  + path/argv escape attempts asserted to be structurally refused).
- P3: the whole loop tested against `FakeSubstrate` with scripted action
  turns (the Phase 1 pattern) — every verb, every re-ask path, every
  grant violation, the atomic-patch failure-leaves-file-untouched
  property, the subprocess timeout/bound paths (injected runner like
  the 2a POST tests).
- P4: the fixture set validated GPU-free (every reference landing
  applies-and-parses); the live measurement is its own evidence doc,
  pre-registered against G4 before the probe runs.
- Every mutating executor gets a mutation-tested pin (the 2a habit).

## 8. Risks

- **The `run` executor executes model-chosen commands.** The grant is
  the entire security boundary; P2's red-team set and the no-shell /
  argv-prefix / scrubbed-env rules get built and reviewed before P3's
  executor ships, and `tasks_enabled=false` keeps it dark by default.
  The spec deliberately does not claim OS-level sandboxing (no
  namespaces/seccomp in v1) — it claims grant-scoping and says so.
- **G4 may demote everything** — pre-registered as an acceptable
  outcome.
- **Codec choice interacts with the 2a window**: the verb card +
  accumulated observations must fit the measured window; long tasks
  hit the degradation ladder. P3's prompt renderer reuses 2a's
  refuse-with-arithmetic rather than truncating.
- **Canonicalization edge cases** (a path under a root that doesn't
  exist yet — a `patch` creating a new file): the check canonicalizes
  the *parent* and confirms the parent is within a write root, so a
  new file in a granted directory is allowed while `..` escape is not.
  P2 pins this explicitly.

## 9. Deliverable order

1. Plan + execute **P1** (codec + validators, pure).
2. Plan + execute **P2** (grants, pure) — red-team fixtures land here.
3. Plan + execute **P3** (task loop + executors + HTTP, `tasks_enabled`
   gate, FakeSubstrate-tested).
4. Plan + execute **P4** (G4 probe + live landing measurement + evidence).
5. VTT acceptance runs in parallel from the start (separate, against
   existing `/v1`).
