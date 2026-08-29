# G5-v5 baselines — four models, two identical boots each (header committed BEFORE any boot)

**Date:** 2026-08-29. **Protocol:**
`docs/superpowers/evidence/2026-08-29-g5v5-protocol.md` (binding);
instrument `codec-tasks-v5-mixed` (frozen at its authoring commit, sha256
`bf2db8ac3c645f37e681412f4606c50a3ecd52d0548a2c09be7c18641ca0ae13`)
under `bloomery-task-envelope-v5`; G4 on `codec-tasks-v1` unchanged,
run in the same boots as corroborating context. Run under Brice's
2026-08-29 delegation (turn-6 plan header; ledger R2: GPU-hygiene check
before each boot, a held GPU is a STOP).

## Pre-registration (the anchors, declared before any boot)

**Boot 1 is the anchor for each model**, declared here before that
model's first boot. Two identical boots per model; byte-identity of
verdicts, `done` texts, and declared attributes across the two boots is
reported (greedy Vulkan decode is not guaranteed bit-identical across
launches — a recorded box fact — so divergence is reported, never
silently averaged). Boot order (serial, one daemon at a time, port
8497, fresh scratch `data_dir` per boot under the turn-6 SDD tree):

| # | model (API name) | artifact | expected digest (sha256 of the file) | geometry |
|---|---|---|---|---|
| 1–2 | `qwen36-reap48-ours` (untrained) | `~/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf` | `90e2181e…` (full value asserted at boot) | REAP-48 line: `ctx_overhead_mib = 512`, no KV override |
| 3–4 | `qwen36-reap48-flywheel5` | `~/flywheel5/qwen36-reap48-flywheel5-Q4_K_M.gguf` | `7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd` | REAP-48 line, same |
| 5–6 | `qwen3:14b` (stock) | `/mnt/extra/ollama-models/blobs/sha256-a8cc1361…` | `a8cc1361f3145dc01f6d77c6c82c9116b9ffe3c97b34716fe20418455876c40e` (verified by sha256sum this session) | 14B line: plain config (the retained `target/fw4-live` geometry) |
| 7–8 | `qwen3-14b-flywheel4` | `~/flywheel4/qwen3-14b-flywheel4-Q4_K_M.gguf` | `5de74418…` (full value asserted at boot) | 14B line, same |

Config template per boot (only the model stanza, `data_dir`, and — for
the REAP-48 line — `ctx_overhead_mib` vary): `tasks_enabled = true`,
`envelope = "v5"`, `g5_probe = true`, `[tier] enthusiast-16gb`,
`[assay] enabled, python3, probe_timeout_secs = 1800`; **no `[memory]`
table** (default off — every frozen instrument runs memory-off).

Per boot, committed beside this doc: the boot journal, the tasks
journal, and the `tools.evidence.recompute` JSON (G4 + G5-v5 landing
per class with Wilson and the decided/provisional flag + the three
declaration endpoints by family + the carried secondaries). Anatomy and
every number in the results sections below come from those JSONs, never
memory. The served digest is read from `/status` and matched to the
artifact sha; `readlink /proc/<pid>/exe` confirms the featured binary.

*(Results are appended below by later commits, model by model, after
each pair of boots.)*
