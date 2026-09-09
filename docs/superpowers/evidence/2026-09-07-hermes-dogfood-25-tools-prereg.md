# Pre-registration — hermes-on-bloomery dogfood, 25-tool set (2026-09-07)

Written BEFORE the daemon was booted. Nothing below changes after the first number is read.

## Subject and lens

- bloomery master `3866da0` (== origin), daemon rebuilt `--release` this session
  (binary mtime 2026-09-07 08:24:53). Includes the 08-31 `/v1` honesty fix,
  `DELETE /agents/{id}` (PR #22) and `max_placeable_tokens` refusals (PR #23) —
  none of which the 08-31 runs had.
- adapter `adapters/openai-tools/` at the same commit, 132/132 unit tests OK this session.
- model `qwen36-reap48-ours` UNTRAINED Q4_K_M — the SAME lens as the 08-31
  measurement arc (NOT flywheel7), so numbers are comparable.
- daemon config = `dogfood-bloomery.toml` (byte-identical to the 08-31
  `measure-bloomery.toml` except `data_dir` → fresh `/mnt/extra/bloomery-dogfood-20260907`
  and the comment line). tier enthusiast-16gb, ctx_overhead_mib 512, overhead 1024, tasks off, assay off.
- adapter config = `adapter-config.json` byte-identical to 08-31 (`window_cap` 30000,
  `max_tokens` 768, `max_tokens_cap` 4096, port 8011).
- hermes = live install `~/.hermes/hermes-agent` (gateway PID 1169016 NOT touched,
  `~/.hermes/config.yaml` NOT touched), run under a FRESH `HERMES_HOME` =
  `scratchpad/dogfood/hermes-home` whose `config.yaml` is a copy of the 08-31 scratch profile
  (provider ollama, base_url adapter). Toolset = hermes-cli DEFAULT (no `-t` flag) — this is
  the 25-tool set captured on 08-31 (`capture.jsonl`, n_tools=25). Non-interactive `-q`,
  tools executing (no `--safe-mode`), no `--yolo` unless a run stalls on an approval
  prompt (if so: recorded, and the stalled run is INFRASTRUCTURE, rerun from zero).
- sampler: bloomery substrate is `LlamaSampler::greedy()` — pinned by the daemon, not configurable.

## What is new vs 08-31

The 08-31 PASS (run 4: 0 resets, 1 agent, deltas 32/212/54) was measured with a
4-tool set (`-t file`: patch/read_file/search_files/write_file). The 25-tool set was only
ever driven BEFORE the streaming + retry + identity fixes, and that run was PARTIAL
(409 on retry). So this is the first time the fixed adapter meets hermes's real default toolset.

## Task (same shape as 08-31, 25 tools instead of 4)

Prompt, verbatim, one per run with a per-run directory:

> Create the directory /tmp/dogfood-20260907/run<N>/ and write three files in it:
> a.txt containing exactly "alpha", b.txt containing exactly "beta", c.txt containing exactly "gamma".
> Then list the directory and tell me how many files you created.

Three runs, N = 1,2,3, each a fresh hermes session (`-q`), same adapter process across
all three (so each run is a new adapter session → new bloomery agent → tests that the
one-context tier can turn over agents between sessions).

## Endpoints (decided by the point estimate; no reruns after reading)

- **E1 functional** — after each run, the three files exist with exact contents
  (`alpha`/`beta`/`gamma`, trailing newline tolerated). PASS = 3/3 runs.
- **E2 prefill-once** — per run, from the adapter's per-request log: exactly ONE
  request with `prompt_tokens` ≥ 10,000 (the 25-tool preamble), every other request
  in that session < 1,000, `reset:false` on all but possibly the first, ONE agent
  per session. PASS = 3/3 runs. (08-31's 4-tool run had deltas 32/212/54.)
- **E3 preamble size** — the preamble `prompt_tokens` recorded (08-31 estimate ≈14,077).
  Must be < `window_cap` 30,000 or the adapter refuses honestly (that would be a FAIL of
  the tier claim "25-tool set fits", not of the adapter).
- **E4 parse** — every per-request `outcome` is `ok`; every tool the model invoked was
  an actual tool from the 25 (hermes would error otherwise). Count of tool calls recorded.
  A non-`ok` outcome that is a bloomery refusal (413/409 with arithmetic) is recorded
  as such, not as a parse failure.
- **E5 daemon health** — after all runs, `GET /status` answers within 5 s and the
  journal shows every `InferStarted` paired with a terminal infer event. A wedge is
  the pager-lock-across-infer class → INFRASTRUCTURE, recorded, run ends (no rerun).
- **E6 retries** — count of byte-identical retried requests hermes sends (adapter log).
  Informational: the replay path must serve them WITHOUT a reset (E2 covers it).

## Kill / infrastructure criteria

- Daemon fails to boot, or ModelLoaded digest ≠ the file at
  `/home/brice/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf` → infrastructure, fix, rerun from zero.
- Any run killed externally before its final answer → discarded, rerun from zero; never spliced.
- Wall clock cap per run: 15 min (08-31 runs completed in ~1–2 min each).

## Verdict rule

PASS = E1 3/3 ∧ E2 3/3 ∧ E4 all ok ∧ E5 healthy. Anything else = PARTIAL with the
failing endpoint named. E3/E6 are recorded numbers, not gates.
