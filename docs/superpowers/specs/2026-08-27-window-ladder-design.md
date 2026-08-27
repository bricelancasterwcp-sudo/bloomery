# The window ladder — fixed scope degradation as task-loop client behavior on 413

**Date:** 2026-08-27
**Status:** Approved in conversation (rulings: activation = opt-in per
`TaskSpec` field, default off, every frozen instrument untouched; rungs =
memory first, then old-entry elision — goal, grant line, and verb card never
degrade; model marker = one pinned head note, rendered only when ≥1 entry
was actually elided; ledger = `rung` on `Event::TaskStep` and
`TaskStepRecord` with a serde default of 1; protocol = the pager stays the
only measurer, every render walks the ladder from rung 1, `Budget` refusals
stay terminal).
**Lineage:** robigo's degradation ladder
(github.com/bricelancasterwcp-sudo/robigo, `src/robigo/context/scope.py`
`Scope.degrade`, spec section 3 + the 2026-08-09 amendments): a FIXED
ladder, not a heuristic, so the result is reproducible and testable without
a model; out-of-range steps raise instead of silently clamping; and the
whole-branch review finding (2026-08-09, `_select_rung`) — **measurement is
the authority in both directions**: estimates can both accept a rung that
does not fit and refuse a rung that does, and a step-down-only search can
never step back up, so every rung is tested for real, ascending, every
turn. robigo's `RunRecord.rungs` is the honesty instrument: only the
per-turn rung sequence can tell a run that silently degraded from one that
never left rung 1. Bloomery lineage: the g4 protocol Amendment 1
(`docs/superpowers/evidence/2026-08-15-g4-protocol.md` §9) — mid-task
`PromptTooLarge` is the scored `WindowExhausted` terminal, which this
ladder reaches later, never redefines; the memory organ
(`2026-08-26-memory-organ-design.md` §4/§7) as the pattern for adding an
optional `TaskSpec` input whose absence renders byte-identical bytes; the
pager's refuse-never-truncate rule (`pager.rs` module docs), which this
design preserves — the pager still refuses with arithmetic, and what
changes is that the CLIENT responds by explicitly, ledgered-ly re-scoping
its own prompt instead of dying. Adopted as recommendation #2 of the
2026-08-27 portfolio-review assessment.

## 1. What this builds and why

Today a `run_task` whose prompt outgrows the agent's measured window dies:
`pager.infer` refuses with `PagerError::PromptTooLarge` (HTTP 413 on the
native surface) and `propose_action` maps that straight to the terminal
`TaskStatus::WindowExhausted`. That is the correct floor — refuse, never
truncate — but it wastes every task whose prompt would fit if the client
honestly shrank what it asks the model to look at. robigo proved the shape
that works: a fixed ladder of progressively smaller scopes, walked by real
measurement, refusing only when the smallest rung still does not fit, with
the rung actually used ledgered every turn.

This spec ports that shape into `run_task` as **opt-in client behavior**:
when (and only when) a task opted in, a `PromptTooLarge` refusal makes the
loop re-render the same turn one rung smaller and re-submit, up to rung 4;
only a rung-4 refusal remains terminal. The pager is untouched. The
renderer stays truncation-free in the pager's sense: elision is explicit
(the model is told), fixed (no heuristics), and journaled (the rung is in
every step row).

**Claim discipline.** This slice claims mechanism only: the ladder walks,
lands on the first fitting rung, renders the pinned bytes, and ledgers
honestly. It claims nothing about whether degraded tasks succeed more often
than dead ones — that is a capability claim, and it belongs to a future
frozen instrument with its own pre-registration. No number from this
slice's tests may appear in a capability sentence. Every G4/G5 battery,
codec probe, drift probe, and flywheel run stays ladder-off and
bit-identical to today.

## 2. The ladder

Four rungs, fixed, then refusal. Every rung keeps the goal (the anchor —
the task IS the goal), the grant section, and the verb card: the model must
always see its true contract. The `THINK_PRESEED` and stop-sequence
behavior of the task's `EnvelopeLens` apply unchanged at every rung.

- **Rung 1** — today's rendering, byte-for-byte. The existing
  `render_prompt_from` output with nothing added and nothing removed.
- **Rung 2** — rung 1 with `memory_block` rendered as if `None`. Legal by
  the organ's own law (memory-organ spec §7: total failure must be
  indistinguishable from memory-off); the injected episode is best-effort
  orientation, the first thing to go — robigo's rung 2 drops hop-2
  signatures for the same reason. A task with no memory block renders rung
  2 identical to rung 1; the walk simply refuses it too and moves on
  (robigo has the same property and the same answer: the ladder stays
  fixed).
- **Rung 3** — rung 2, with every transcript entry EXCEPT THE LAST 2
  elided to its header. An entry is one `record_step` appendation — parse
  re-ask diagnostics included — and its full form is `transcript_entry`'s
  pinned `"\n[step {step} {verb}] {outcome}\n{content}\n"`. The elided
  form is that string minus the content line:
  `"\n[step {step} {verb}] {outcome}\n"`. What survives is the record of
  what was done and how it went; what goes is re-obtainable content (the
  model can re-read a file; it cannot re-know what it already tried).
- **Rung 4** — rung 3 with only the LAST 1 entry full.
- **Refusal** — rung 4 still refused → `TaskStatus::WindowExhausted`, the
  same scored terminal Amendment 1 defines, with the pager error's
  arithmetic (`needed_tokens`, `window_tokens`) in the summary, exactly as
  today — just reached four rungs later.

With fewer entries than a rung's full-window, there is nothing to elide
and the rung renders identical to rung 2; the walk refuses through it
naturally. No rung is ever skipped and no rung count is ever computed —
fixed order, every render.

## 3. The head note

At rungs 3 and 4, when at least one entry was actually elided, one line
renders between the verb card's trailing `\n\n` and the transcript:

```
[context note: contents of steps {a}-{b} elided to fit the window; outcomes retained — re-read files if needed]
```

followed by one `\n`. `{a}` is the first elided entry's step number, `{b}`
the last elided entry's step number, always in the `{a}-{b}` form even when
equal — fixed format, no branching. When zero entries were elided the note
does not render at all, not even blank — the memory organ's byte-identity
discipline, applied here: absence adds nothing.

The note exists because the elided form is not self-describing: a
`[step 3 read] ok` with no body could be misread as an empty file. The
note prevents the misread and names the correct recovery (re-read).

## 4. Protocol: who measures, who walks

The pager is the ONLY measurer. The loop never estimates tokens, never
reads the window, never predicts a verdict — it renders a rung and submits
it, and the pager's accept or refuse IS the measurement (its window check
is pre-inference arithmetic, so a refused rung costs no GPU work). This is
robigo's whole-branch-review lesson transposed: an estimate of an estimate
fails in both directions; the authority's own verdict on the real rendered
candidate fails in neither.

**Every render walks from rung 1.** Each attempt (first ask and each parse
re-ask alike) starts at rung 1 and ascends only as far as refusals push
it. A step-down-only ratchet is robigo's named failure mode — "a rung
lower than necessary ... a step-down-only search can never step back up
from" — and re-walking costs at most three refused arithmetic checks. The
transcript only grows, but the walk's cheapness makes the invariant free
to keep unconditionally.

A `PromptTooLarge` that arrives classified from a substrate-side error
after submission (`classify_infer_error`) triggers the same rung-up: a
window refusal is a window refusal regardless of which side measured it.
The ladder is finite, so no loop is possible.

`PagerError::Budget` stays the terminal `BudgetExhausted` at every rung —
the ladder reacts to `PromptTooLarge` and to nothing else. Every other
`infer` failure stays `Error`, per Amendment 1's exact carve-out.

Ladder-off (`window_ladder == false`): the existing single-attempt
behavior, byte-for-byte and status-for-status — the first `PromptTooLarge`
is terminal, exactly today's `propose_action`.

## 5. Activation

`TaskSpec` gains `window_ladder: bool`. Default `false` at EVERY existing
construction site — `api_task.rs`'s `create_task`, the codec probe, the
flywheel factory, `test_support`, every test — so every frozen instrument
constructs, renders, and terminates bit-for-bit as today. `create_task`
accepts an optional `"window_ladder"` request field (absent → `false`) so
live tasks opt in over HTTP; no other surface sets it in this slice.

`render_task_prompt` (the flywheel factory's serving-faithful wrapper) is
untouched and permanently rung-1: the factory renders training pairs
against pinned goldens and may never see a degraded prompt — same
unrepresentability argument as its hardcoded `memory_block: None`.

## 6. The ledger

`Event::TaskStep` gains `rung: u32` with a named serde default returning 1
(`#[serde(default = "default_rung_one")]` — the `default_expect_patch`
pattern, since a bare `#[serde(default)]` would replay old rows as the
nonexistent rung 0): every pre-ladder row replays as rung 1, which is the
truth of what it was. `TaskStepRecord`
gains the same field, and `get_task`'s per-step JSON objects expose it.
The value is the rung whose prompt was ACTUALLY SENT for the attempt that
produced that row — parse-failure rows carry the rung their own failed
attempt used. The per-task rung sequence therefore falls out of existing
journal rows at attempt granularity (finer than robigo's per-turn tuple),
and each intermediate rung-up's arithmetic is already journaled by the
pager's own refusal event. Nothing new is emitted.

## 7. Error handling

- Journal write failures stay task-fatal, unchanged (`pager.rs` rule 4).
- A rung outside 1..=4 reaching the renderer is a programming error and
  panics — robigo's `ValueError` rule: no silent clamping, in either
  direction, ever.
- `WindowExhausted` semantics, scoring, and journal shape are unchanged;
  the ladder only changes how much refusing it takes to get there, and
  only for opted-in tasks.
- The `render_prompt` docstring's "deliberately does no windowing or
  truncation" paragraph is updated to distinguish silent truncation
  (still forbidden, still the pager's law) from this explicit, journaled,
  fixed-ladder re-scope (the client's honest response to refusal).

## 8. Testing

All `FakeSubstrate`, GPU-free, TDD (red → green per test):

1. **Ladder-off identity** — a `window_ladder: false` task hitting
   `PromptTooLarge` terminates `WindowExhausted` on the first refusal;
   existing render goldens (`task_render_test.rs`,
   `memory_render_test.rs`) pass untouched, pinning rung-1 bytes.
2. **Ascending walk** — tiny-window pager: the loop lands on the first
   fitting rung; the substrate sees exactly the pinned rung-N bytes
   (golden per rung, head note included at 3/4).
3. **Re-walk from rung 1** — a later attempt renders rung 1 first even
   after an earlier attempt degraded (asserted via the pager's journaled
   refusal sequence).
4. **Rung ledger** — journal rows and `TaskStepRecord`s carry the sent
   rung; a pre-ladder journal row replays with `rung == 1`
   (`journal_test.rs` compat pin); `get_task` exposes it.
5. **Terminal refusal** — rung 4 refused → `WindowExhausted` with the
   pager arithmetic in the summary.
6. **Head note law** — renders at rungs 3/4 only when ≥1 entry elided;
   never at rungs 1/2; never when the elision set is empty.
7. **Memory rung** — rung 2 bytes equal the same task rendered
   memory-off; a memory-less task's rung 2 equals its rung 1.
8. **HTTP default-off** — `create_task` without the field builds
   `window_ladder: false`; with `true` the task degrades.
9. **Mutation spot-check** — the elision boundary (last-2 vs last-1) and
   the rung-refusal boundary (4 vs 5) each kill a hand-applied mutant,
   per house discipline.

Acceptance: full workspace `cargo test` green, zero ignored; featured
binary rebuilt after the last test run (box rule).
