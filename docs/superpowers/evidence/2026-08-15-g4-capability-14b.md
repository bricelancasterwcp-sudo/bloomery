# G4 capability window, rung 1 — qwen3:14b

**Date:** 2026-08-15 (verdict ~17:20 local; probe wall ~95 min incl. POST)
**Status:** measured; the pre-registered decision applied
**Protocol:** `2026-08-15-g4-protocol.md` including **Amendment 1 (§9)**,
recorded and committed BEFORE this run (see attempt history below).
**Gate:** `docs/gates.md` G4 — unamended.

## Verdict

**1 / 20 fixtures landed. Wilson 95% [0.009, 0.236]. Not provisional (the
interval sits entirely below the 0.80 floor). qwen3:14b is demoted to
read-only verbs.**

```json
{"event":"CodecVerdict","model":"qwen3:14b","fixture_set":"codec-tasks-v1",
 "codec":"search_replace","landed":1,"n":20,
 "interval95":[0.008881448800795402,0.23613119344674208],
 "provisional":false,"mutating_verbs":false,
 "detail":"applies_and_parses under bloomery-task-envelope-v1; codec from profile"}
```

`/status` after the verdict: `patch_codec: "search_replace"`,
`mutating_verbs: false`, `codec_gate` populated with the same numbers.

## Attempt history (all three attempts are part of this rung's record)

1. **Attempt 1 (pre-branch, master @ 64d5b95):** refused at placement —
   the carried-debt **item 7** asymmetry, live (window VRAM-bound at
   27,913 tokens; placement needed exactly `ctx_overhead` more than the
   budget the window was sized against). Motivated the item-7 fix
   (spec §3b); the `Refusal` line is quoted in `docs/CARRIED-DEBT.md`.
2. **Attempt 2 (post item-7 fix):** POST succeeded (the fix live-verified),
   fixture 1 scored, fixture 2 **window-exhausted** — verbatim substrate
   refusal: `refusing: 24533 cached + 593 prompt + 1024 requested tokens
   exceed the window of 25600 tokens` — and §3-as-written classified the
   resulting `TaskStatus::Error` as an infrastructure abort. Motivated
   **protocol Amendment 1** (§9, Brice-approved, committed before any
   re-run): mid-task window exhaustion is a *scored* outcome
   (`WindowExhausted`), the `BudgetExhausted` shape; `Error` remains an
   abort. No verdict existed at the abort; no measurement was spliced.
3. **Attempt 3 (this measurement):** full 20-fixture run, zero aborts.

## Subject

- **Model:** `qwen3:14b` (ollama blob `sha256-a8cc1361…`, 9.3 GB), full
  offload (`n_gpu_layers` default), fits the card.
- **Box/tier:** `enthusiast-16gb`, `emulated = false` — RTX 5080 16 GB,
  Vulkan. Free-VRAM preflight 14,874 MiB; no competing GPU compute.
- **bloomery:** branch `feat/partial-offload-capwindow` @ `80e719a`
  (item-7 fix + per-model tuning + Amendment-1 terminal; whole-branch
  review completed before this run).
- **assay (boot POST):** pinned at `74c5b71` via git-archive + PYTHONPATH,
  as rung 0.
- **Sampler:** greedy (substrate-pinned). Prompt path: raw completion, no
  chat template — the lens every model gets; qwen3's thinking behavior is
  measured as-is.
- **Lens note (boot-time window):** the agent window depends on the static
  boot-time free-VRAM read (~25.6k tokens this boot). Attempt 2's
  window-exhaustion did not recur in attempt 3 — a slightly larger boot
  budget let the step budget bind first (13 StepsExhausted). The
  boot-time budget is part of the lens and is recorded here.

## Per-fixture record

Recomputed from the committed journal's `CodecFixture` events: **20 rows,
1 landed — equals the verdict's `landed/n`; the recomputability obligation
holds.** Tally: 13 `StepsExhausted`, 5 `SearchNotFound` (patch emitted but
its search block didn't byte-match the file), 1 grant violation, 1 landed.

| fixture | result | step records | terminal / detail |
|---|---|---|---|
| `py-mean-off-by-one` | not landed | 16 | grant violation (patched a path outside the granted root) |
| `py-max-wrong-comparison` | not landed | 18 | StepsExhausted |
| `py-countdown-range-off-by-one` | not landed | 16 | SearchNotFound (python lens) |
| `py-discount-wrong-operator` | not landed | 18 | StepsExhausted |
| `py-firstlast-wrong-index` | not landed | 14 | SearchNotFound (python lens) |
| `py-shipping-wrong-boolean` | not landed | 4 | SearchNotFound — search block missing leading indentation |
| `py-greeting-wrong-fstring-var` | **landed** | 16 | patched (lens: python) |
| `py-cart-total-missing-tax` | not landed | 18 | StepsExhausted |
| `py-inventory-restock-threshold` | not landed | 18 | StepsExhausted |
| `py-validator-password-length` | not landed | 18 | StepsExhausted |
| `txt-listen-port-mismatch` | not landed | 18 | StepsExhausted |
| `txt-db-connection-string` | not landed | 18 | StepsExhausted |
| `txt-retry-count-wrong` | not landed | 18 | StepsExhausted |
| `txt-readme-wrong-command` | not landed | 16 | StepsExhausted |
| `txt-changelog-wrong-version` | not landed | 16 | SearchNotFound (plaintext lens) |
| `txt-email-template-wrong-name` | not landed | 18 | StepsExhausted |
| `txt-nginx-upstream-mismatch` | not landed | 18 | StepsExhausted |
| `txt-support-doc-wrong-url` | not landed | 18 | StepsExhausted |
| `txt-env-wrong-timeout` | not landed | 18 | StepsExhausted |
| `txt-release-notes-wrong-date` | not landed | 4 | SearchNotFound (plaintext lens) |

## The capability window so far

| model | landed | Wilson 95% | dominant failure mode |
|---|---|---|---|
| qwen2.5-coder 7B Q8 | 0/20 | [0.000, 0.161] | never emitted a valid action (`NoAction` ×142, `MultipleActions` ×82) |
| qwen3:14b | 1/20 | [0.009, 0.236] | engages the envelope; patches fail byte-exact search matching (indentation drops — robigo's measured 7B failure mode, one size up) |

The step from 7B to 14B is real but small under this envelope: the 14B
emits genuine `<action>` patches where the 7B produced none, and one
landed — but `search_replace`'s byte-exactness (indentation included)
defeats it. Both verdicts are non-provisional demotions; the recorded
escalations remain the 27B rung (next) and the fine-tune flywheel.

## Enforcement

Same as rung 0: read-only verb card + structural dispatch refusal for this
model, per boot, until a run clears the floor.

## Caveats

- Boots-only; greedy decoding; N=20 granularity; raw-completion prompt
  path (no chat template) — all as rung 0.
- The window's boot-time VRAM dependence (lens note above) means attempt-2
  style window exhaustion can reappear on a tighter boot; Amendment 1 now
  scores it instead of aborting.
- The single grant violation is a live positive for the capability
  boundary, as in rung 0.

## Committed artifacts

- `2026-08-15-g4-capability-14b-journal.jsonl` (1,097 events: POST bracket,
  20 `CodecFixture`, 1 `CodecVerdict`, agent lifecycle)
- `2026-08-15-g4-capability-14b-tasks.jsonl` (318 `TaskStep` rows)
