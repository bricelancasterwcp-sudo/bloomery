# G5 baselines — stock qwen3:14b and qwen3-14b-flywheel1 (before turn 2)

**Date:** 2026-08-16. **Gate:** G5 (`2026-08-16-g5-protocol.md`), fixture
set `codec-tasks-v2-mixed` (frozen, incl. its recorded txt-01 amendment),
envelope-v3, greedy, advisory. Both runs also exercised the G4 probe
first (same boot); journals committed beside this doc. These anchor
flywheel2's delta and are measurements in their own right.

## Verdicts

| model | patch class | refuse class | done_trust |
|---|---|---|---|
| qwen3:14b (stock) | 4/10 [0.168, 0.687] decided fail | 2/10 [0.057, 0.510] decided fail | **false** |
| qwen3-14b-flywheel1 | **10/10** [0.722, 1.0] provisional pass | 7/10 [0.397, 0.892] provisional, below floor | **false** |

## What the anatomy says

- **Stock** fails refusal mostly by never finishing: 7 of 8 misses are
  leg (c) (no terminal `Done` — it thrashes), 1 leg (a). Its patch-class
  4/10 is consistent with its 7/20 on codec-tasks-v1.
- **flywheel1 generalizes**: 10/10 on the NEW, differently-authored
  patch fixtures — the turn-1 habit was not template overfit. And with
  ZERO refusal training it already refuses correctly 7/10 (read → sees
  no defect → honest done), vs stock's 2/10 — the read-first habit
  partially transfers to honesty.
- **flywheel1's entire remaining gap is one behavior**: all 3 refuse
  misses are leg (a) — *it patched a correct file* (both defect-absent
  lenses, incl. the amended txt-01): when the goal asserts a plausible
  defect, its trained comply-instinct overrides what it just read.
  Exactly the delta turn 2's defect-absent corpus targets.

## Notes

Per protocol §3, at n=10 every pass is provisional; both baselines'
failing classes are what they are. The G5 instrument behaved correctly
on its first live outings: per-class verdicts, no blending, `done_trust`
false for both, G4 comparability untouched (the same boots re-ran the
G4 probe on codec-tasks-v1 with results consistent with prior rungs).
