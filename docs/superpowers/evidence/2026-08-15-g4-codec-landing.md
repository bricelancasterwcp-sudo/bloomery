# G4 live codec-landing measurement — qwen2.5-coder 7B Q8

**Date:** 2026-08-15 (boot 04:40:45 local, verdict ~05:45 local — ~65 min wall
including the assay POST and 20 fixtures through the real task loop)
**Status:** measured; the pre-registered decision applied
**Protocol:** `2026-08-15-g4-protocol.md` (pre-registered 2026-08-15, before the
instrument existed). No amendment was made before, during, or after this run.
**Gate:** `docs/gates.md` G4 — landing ≥80% keeps mutating verbs, else demotion.

## Verdict

**0 / 20 fixtures landed. Wilson 95% interval [0.000, 0.161]. Not provisional
(the interval sits entirely below the 0.80 floor). The model is demoted to
read-only verbs (`read`/`find`/`done`).**

This is the umbrella spec §8's pre-registered honest outcome — "every local 7B
demoted" — materialized in full, and it is a valid measurement, not a failure
of the run. Read-only agents with honest refusal remain useful; the recorded
escalations are (a) a bigger model (qwen3.8:27b is the standing candidate —
exactly G4's capability-window question) and (b) the black-oxide fine-tune
flywheel.

```json
{"event":"CodecVerdict","model":"qwen2.5-coder:7b-instruct-q8_0",
 "fixture_set":"codec-tasks-v1","codec":"search_replace","landed":0,"n":20,
 "interval95":[0.0,0.16112515805281938],"provisional":false,
 "mutating_verbs":false,
 "detail":"applies_and_parses under bloomery-task-envelope-v1; codec from profile"}
```

`/status` after the verdict: `profiled: true`, `patch_codec: "search_replace"`,
`mutating_verbs: false`, `codec_gate` populated with the same numbers.

## Subject

- **Model:** `qwen2.5-coder:7b-instruct-q8_0`, weights = the ollama blob
  `sha256-24b532e527…ca54a1` (verified against `ollama show … --modelfile`
  before the run — served identity, not liveness).
- **Box/tier:** `enthusiast-16gb`, `emulated = false` — RTX 5080 16 GB,
  Vulkan (llama.cpp), the same box and tier as G2 and the assay profiles.
- **bloomery:** branch `feat/phase2bc-p4-codec-gate` @ `c40198c` (the final
  reviewed instrument; the whole-branch review completed BEFORE this run).
- **assay (boot POST prober):** pinned at `74c5b71` (last v1.4-behavior
  commit), extracted via `git archive` and supplied via `PYTHONPATH` — pinned
  deliberately because the assay working tree is mid-v1.5 development; the
  instrument version is part of the lens.
- **Sampler:** the substrate's pinned decoder is **greedy**
  (`LlamaSampler::greedy()`, `bloomery-substrate/src/llama.rs`) —
  deterministic, no temperature dimension.
- **Config:** `tasks_enabled = true`, `assay.enabled = true`,
  `overhead_mib = 1024`, `ctx_overhead_mib = 384`, fresh
  `data_dir = ~/.cache/bloomery-g4`. Free VRAM preflight: 13,651 MiB ≥ the
  12 GiB floor; no competing GPU compute processes.

## Instrument (the lens, named)

- Fixture set **`codec-tasks-v1`** (N=20: 10 `python` + 10 `plaintext`
  lenses), embedded in the daemon, structurally validated GPU-free (every
  reference landing applies-and-parses through the real `land()`).
- Envelope: **`bloomery-task-envelope-v1`** — the real P3 task loop (verb
  card + exactly-one-action rule + max 2 re-asks per step), real executors,
  real grants (read+write on the fixture dir only, no commands).
- Patch codec: **`search_replace`, "codec from profile"** — selected per
  protocol §4 from the boot POST's assay profile (`codecs` grid, grade
  `small`, `lands_applies` comparison).
- Parameters (protocol §2): `max_steps = 6`, per-fixture agent budget
  30,000 tokens, fresh agent + fresh scratch dir per fixture.
- Scoring (protocol §3): landed iff a `patch` step succeeded AND the declared
  target file's bytes changed. Decision (protocol §5): `landed*5 >= n*4` on
  the point estimate; Wilson 95% recorded; provisional iff the interval
  straddles 0.80.

## Per-fixture record

Recomputed from the committed journal's `CodecFixture` events
(`grep '"event":"CodecFixture"' 2026-08-15-g4-codec-landing-journal.jsonl`):
**20 rows, 0 landed — equal to the verdict's `landed/n`, the recomputability
obligation holds.**

| fixture | result | step records | terminal |
|---|---|---|---|
| `py-mean-off-by-one` | not landed | 16 | StepsExhausted |
| `py-max-wrong-comparison` | not landed | 18 | StepsExhausted |
| `py-countdown-range-off-by-one` | not landed | 18 | StepsExhausted |
| `py-discount-wrong-operator` | not landed | 18 | StepsExhausted |
| `py-firstlast-wrong-index` | not landed | 18 | StepsExhausted |
| `py-shipping-wrong-boolean` | not landed | 18 | StepsExhausted |
| `py-greeting-wrong-fstring-var` | not landed | 18 | StepsExhausted |
| `py-cart-total-missing-tax` | not landed | 18 | StepsExhausted |
| `py-inventory-restock-threshold` | not landed | 18 | StepsExhausted |
| `py-validator-password-length` | not landed | 18 | StepsExhausted |
| `txt-listen-port-mismatch` | not landed | 18 | StepsExhausted |
| `txt-db-connection-string` | not landed | 18 | StepsExhausted |
| `txt-retry-count-wrong` | not landed | 18 | StepsExhausted |
| `txt-readme-wrong-command` | not landed | 1 | Done |
| `txt-changelog-wrong-version` | not landed | 16 | StepsExhausted |
| `txt-email-template-wrong-name` | not landed | 18 | StepsExhausted |
| `txt-nginx-upstream-mismatch` | not landed | 18 | StepsExhausted |
| `txt-support-doc-wrong-url` | not landed | 18 | StepsExhausted |
| `txt-env-wrong-timeout` | not landed | 18 | StepsExhausted |
| `txt-release-notes-wrong-date` | not landed | 18 | StepsExhausted |

("step records" counts journaled `TaskStep` rows — up to 6 loop steps × 3
parse attempts each = 18; a 16 means one step needed fewer attempts.)

## What actually happened (from the committed `tasks.jsonl`, 339 steps)

The failure is the model's envelope behavior, not the fixture content:

| outcome class | count |
|---|---|
| `NoAction` (turn contained no `<action>` block) | 142 |
| `MultipleActions` (turn contained several blocks — exactly-one violated) | 82 |
| terminal `unparseable after 2 re-asks` | 112 |
| clean `read` (envelope parsed, executor ran, 181 bytes) | 1 |
| `done` ("no changes needed" — `txt-readme-wrong-command`, no patch attempted) | 1 |
| `grant violation` — the model emitted `run ["cargo","test",…]`; **no commands were granted and the applier refused it structurally** | 1 |

Instrument-honesty reading: the one clean `read` and one clean `done` prove
the envelope path accepts well-formed actions end-to-end on this exact run —
so 0/20 measures the model's inability to reliably emit exactly one valid
action under this envelope (greedy decoding, unfamiliar grammar), not a
broken parser. This echoes robigo's measured 7B reality and assay's
2026-08-12 lesson that landing is a property of model × codec × directive ×
sampler — under assay's own `--quick` presentation this same model measured
well enough to select `search_replace`; under the task envelope it landed
nothing. The lens is named in every record for exactly this reason.

The single grant-violation row is a live positive for P2/P3: a model-chosen
mutating command was refused by the capability boundary during a real run.

## Enforcement now in force (this boot and every boot until a model passes)

- Task creation resolves `mutating_verbs: false` for this model; the verb
  card offers `read`/`find`/`done` only, and a `patch`/`run` action would be
  structurally refused with `verb unavailable: mutating verbs demoted (gate
  G4)`.
- Demotion is per-boot state, re-measured at every boot (CARRIED-DEBT item
  11).

## Caveats

- **Boots-only**: the gate re-measures at boot; no drift re-probe between
  boots (same honest limit as the assay POST, CARRIED-DEBT item 6).
- **Greedy decoding** is the pinned sampler; a different sampler is a
  different lens and would need its own record.
- **N=20 granularity** (5 pp); moot here — the interval is decided, not
  provisional.
- **GPU co-tenancy**: the box is shared, but preflight verified no competing
  compute process; the run was not contended.
- Orphan-row rule (module docs): the `CodecFixture` rows here are readable as
  a rate because a matching `CodecVerdict` exists in the same journal.

## Committed artifacts

- `2026-08-15-g4-codec-landing-journal.jsonl` — the boot journal (1,139
  events: POST, provisional-admission bracket, 20 `CodecFixture`, 1
  `CodecVerdict`, agent lifecycle).
- `2026-08-15-g4-codec-landing-tasks.jsonl` — every `TaskStep` of all 20
  fixture runs (339 rows).
