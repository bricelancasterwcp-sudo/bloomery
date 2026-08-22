# tools/evidence

Recomputes a boot's G4/G5 verdicts and secondary endpoints from its committed
journals (`CodecFixture`/`CodecVerdict`/`CodecVerdictMixed` rows) and
`TaskStep` rows, joined against the frozen fixture TOML. Stdlib only.

It **reports; it never decides.** The daemon's own gate is the sole
authority on pass/fail — this tool exists to catch drift between the
committed evidence prose and what the journals actually contain, not to
gate anything itself.

## CLI

```bash
python3 -m tools.evidence.recompute \
  --journal J.jsonl --tasks T.jsonl \
  --g5-fixtures crates/bloomery-daemon/fixtures/codec-tasks-v4-mixed.toml \
  [--g4-set codec-tasks-v1] [--json out.json]
```

Prints the JSON report to stdout; exits 2 if the CodecFixture<->TaskStep
join reports any violations, else 0.

## Turn-4 pins

`tools/evidence/tests/test_recompute_turn4.py` pins this tool against the
committed turn-4 journals: flywheel4-g4 (20/20 landed), flywheel4-g5
(g4 20/20, g5 patch/refuse 16/16), g5v4-flywheel3 (g4 20, g5 15/16,
5 grant-violation rows), g5v4-stock14b (g4 6, g5 5/8, 42 grant-violation
rows). Numbers were re-derived with `jq` on 2026-08-22 and match the
committed evidence documents exactly.
