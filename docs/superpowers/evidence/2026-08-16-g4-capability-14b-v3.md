# G4 — qwen3:14b under envelope-v3: 7/20, demoted

**Date:** 2026-08-16 (verdict ~08:05 local, ~40 min wall incl. POST)
**Protocol:** `2026-08-15-g4-protocol.md` incl. Amendments 1-3. Lens:
`bloomery-task-envelope-v3`. bloomery @ `0d6ae6b` (PR #9 branch, as the
Q3-v3 pass); assay pinned `74c5b71`; greedy; full offload (standard
attention — no KV override needed; GGUF-derived charge is accurate).

## Verdict

**7 / 20 landed. Wilson 95% [0.181, 0.567]. Non-provisional demotion.**
Recomputation from the committed rows: 7/20 — matches.

```json
{"event":"CodecVerdict","model":"qwen3:14b","fixture_set":"codec-tasks-v1",
 "codec":"search_replace","landed":7,"n":20,
 "interval95":[0.18119182410108212,0.5671457233147638],
 "provisional":false,"mutating_verbs":false,
 "detail":"applies_and_parses under bloomery-task-envelope-v3; codec from profile"}
```

## What the lens ladder did for the 14B (v1 → v3: 1/20 → 7/20)

The envelope diseases are now COMPLETELY gone for this model too: across
all 76 steps, zero `NoAction`, zero `MultipleActions`, zero re-ask
terminals, zero `WindowExhausted`. What remains is pure model behavior:

- **51 of 76 steps are `SearchNotFound`** — it patches from imagination.
  Only **2 `read` steps in the entire run**: it fabricates the file's
  bytes from the goal, retries with new guesses, and exhausts its steps
  (8 fixtures at the full 6; the 7 landings took the minimal 2).
- **All 7 landings are plaintext; zero python.** The guessable
  config/prose one-liners land; anything whose exact bytes can't be
  inferred from the goal text does not. 4 grant violations (patched
  invented paths — structurally refused, boundary held).

## The capability window, final table (this arc)

| model | v1 | v2 | v3 | v3 verdict |
|---|---|---|---|---|
| qwen2.5-coder 7B Q8 | 0/20 | — | — | (not re-gated) |
| qwen3:14b | 1/20 | — | 7/20 | demoted — blind patching |
| qwen3.8-27b Q3 | 0/20 | 10/20 | **20/20** | **PASS** |

Under the honest envelope, the difference between the models is now
crisp and behavioral: the 27B reads before patching and lands
byte-exactly; the 14B does not read. No envelope change can supply the
read-first discipline — that is model behavior, and it is exactly what
the fine-tune flywheel (black-oxide) trains. The 14B is the natural
first flywheel candidate: fast, fully-offloaded, and failing on one
specific, trainable habit.

## Caveats

Boots-only; greedy; N=20; per-(model, envelope) verdicts; journals
committed beside this doc (recomputable).

## Committed artifacts

- `2026-08-16-g4-capability-14b-v3-journal.jsonl`
- `2026-08-16-g4-capability-14b-v3-tasks.jsonl`
