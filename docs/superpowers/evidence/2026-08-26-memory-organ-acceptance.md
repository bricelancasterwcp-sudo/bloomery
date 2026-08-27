# Memory-organ slice 1 — live acceptance: mint, injected repeat, stranger and drift silence

**Date:** 2026-08-26 (evening boot; Brice's go: "gpu free"). **Branch:** `memory-organ`
@ `d69be3f` (worktree `.worktrees/memory-organ`), the whole-branch-reviewed tip.
**Spec:** `docs/superpowers/specs/2026-08-26-memory-organ-design.md` §8 (the four-arm
protocol). **Claims:** MECHANISM ONLY, per spec §1 — no number below is a capability
claim, and the store file's whole-journal byte-identity across re-runs is not a
property anyone may pre-register (`minted_at` is a row property; journal.rs precedent).

## 1. Preflight and merge-readiness

| item | value |
|---|---|
| GPU before boot | compute apps: ptyxis 25 MiB + lact 28 MiB only (desktop residents) |
| daemon processes | none (`ps -eo pid,comm \| grep -w bloomery-daemon` → exit 1) |
| worktree | `d69be3f`, porcelain clean (untracked scratch only) |
| assay pin | `~/workspace/assay` @ `bdb7f92`, clean — the identical pin the flywheel5 battery used |
| merge-readiness | `cargo fmt --check` ✓, `cargo clippy --workspace -- -D warnings` ✓, `cargo test --workspace` all suites ok, exit 0 — run BEFORE the featured build (order law) |
| featured build | `cargo build --release -p bloomery-daemon --features vulkan` in the worktree, 1m53s; `nm -C target/release/bloomery-daemon \| grep -c ggml_vulkan` → 1 |
| model | `/home/brice/flywheel5/qwen36-reap48-flywheel5-Q4_K_M.gguf`, served digest `7020b925c07c…` (matches the turn-5 artifact sha) |

**Boot config, verbatim** (`.superpowers/sdd/2026-08-26-memory-organ/acceptance/bloomery.toml`,
sha256 `676d47b7ab21f778…`): the flywheel5 battery boot-1 config adapted to port `8397`,
a scratch `data_dir` under the acceptance directory (fresh drift home — the production
baseline at `~/.local/share/bloomery` untouched all night), plus the section under test:

```toml
[memory]
enabled = true
```

Launch: the battery's exact recipe (`PYTHONPATH=…/assay/src setsid nohup
target/release/bloomery-daemon --config …/bloomery.toml`), real PID `302688` found via
`ps` (never `$!` — setsid gotcha), `readlink /proc/302688/exe` → the worktree binary,
re-confirmed immediately before the kill.

**Boot verdicts:** `/status.memory` on first poll =
`{enabled: true, episodes: 0, verified: 0, contradicted: 0, parse_errors: 0, disabled_reason: null}`.
Codec gate: G4 **20/20 non-provisional** (`codec-tasks-v1`, search_replace, interval95
[0.839, 1.0]) → mutating verbs granted. Drift: `unmeasured` ×2 (no reference documents
in the fresh scratch home — the expected first-boot shape), `admission_block: null`.

## 2. The workspace

`acceptance/workspace/`: `calc.py` (sha `fecab570…`) with `total(xs)` returning
`sum(xs[:-1])`, and `test_calc.py` (sha `95ca4efd…`), two `unittest` cases. Verified
before any boot with plain python3: `python3 -m unittest` → `FAILED (failures=1)` on
the planted bug, `OK` after the fix. Byte-copies in `acceptance/pristine/`; every
workspace reset = `rm -rf __pycache__` + copy both files back (the pycache purge is
deliberate — stale-bytecode hazard, and `exec_run`'s `env_clear` means
`PYTHONDONTWRITEBYTECODE` cannot be granted).

Pinned goal strings (identity-bearing — the normalized goal text is half the episode id):

- repeat: `fix the off-by-one in calc.py so total includes the last element, then run the granted unittest to verify the fix`
- stranger: `read calc.py and summarize what the total function computes`

Grant (all arms): `{"read_roots":[WS],"write_roots":[WS],"commands":[["python3","-m","unittest"]]}`.

## 3. The arms

Every memory row the boot journaled, verbatim from
`acceptance/evidence-raw/tasks.jsonl` (150 rows total; the eight memory rows):

```json
{"event":"MemoryStamp","id":"a147","task_id":"task-1","mode":"silent","episode_id":null,"candidates_checked":0,"epoch_ms":1787788580099}
{"event":"MemoryMint","id":"a147","task_id":"task-1","episode_id":"70d968d12b233da28a446e7a5d98519687e28dde3086c9275ad394a69d0a57f5","epoch_ms":1787788582133}
{"event":"MemoryStamp","id":"a151","task_id":"task-2","mode":"injected","episode_id":"70d968d12b233da28a446e7a5d98519687e28dde3086c9275ad394a69d0a57f5","candidates_checked":1,"epoch_ms":1787788608968}
{"event":"MemoryStamp","id":"a152","task_id":"task-3","mode":"injected","episode_id":"70d968d12b233da28a446e7a5d98519687e28dde3086c9275ad394a69d0a57f5","candidates_checked":1,"epoch_ms":1787788663680}
{"event":"MemoryMint","id":"a152","task_id":"task-3","episode_id":"70d968d12b233da28a446e7a5d98519687e28dde3086c9275ad394a69d0a57f5","epoch_ms":1787788665674}
{"event":"MemoryStamp","id":"a153","task_id":"task-4","mode":"silent","episode_id":null,"candidates_checked":0,"epoch_ms":1787788686928}
{"event":"MemoryStamp","id":"a154","task_id":"task-5","mode":"silent","episode_id":null,"candidates_checked":1,"epoch_ms":1787788707142}
{"event":"MemoryMint","id":"a154","task_id":"task-5","episode_id":"e140a91a68310c60630320c7593b010043e3e4a0587863afc4e282725e69d998","epoch_ms":1787788708472}
```

**Zero `MemoryContradicted` rows** — correct on every arm, including the infra arm (below).

### Arm 1 — mint (agent a147, task-1) — PASS

Trajectory: `read` (60 B) → `patch` (lens: python) → `run` `ran python3 exit 0` →
`done`, status **Done**. Stamp `silent`/0 (empty store). One episode minted:
`70d968d1…`, status `verified`, `cited_paths` = `[…/workspace/calc.py]` — exactly the
file the task touched; the model never read `test_calc.py`, so it is honestly not part
of the identity. `/status.memory` after: `episodes: 1, verified: 1`.

### Arm 2, attempt 1 — an infra refusal, recorded honestly (agent a151, task-2)

Workspace byte-reset (calc.py back to `fecab570…`). Retrieval matched and the stamp
reads **`injected`** with arm 1's episode id — then the task ended **Error, 0 steps**:
`residency refused: needed 2644467712 B, free 2304 B, reclaimable 0 B`. The default
agent window (~2.5 GiB KV at this model's derived geometry) could not be placed beside
the still-resident arm-1 agent — the pager's tight-tier refusal, not an organ defect.
**The organ's infra exemption held live:** no `MemoryContradicted` row, the episode
stayed `verified` — an unmeasured task retired nothing (spec §5 as amended by the
scored-outcomes ruling). Operator remedy per the tight-tier flow: suspend the stale
agents; every later agent created with `window_cap: 16384` (~335 MiB KV).

### Arm 2, attempt 2 — injected repeat (agent a152, task-3) — PASS

Same byte-reset workspace (verified `fecab570…` before submit). Stamp
**`injected`**, episode `70d968d1…`, `candidates_checked: 1`. Trajectory — note it
differs from arm 1's, the injection is advisory, not a script: `run` (`exit 1` — the
model first re-ran the unittest and confirmed the planted failure) → `read` → `patch`
→ `run` `exit 0` → `done`, status **Done**. Then the refresh: a second `MemoryMint`
with the **same** episode id (the task identity — goal hash + fingerprint set —
unchanged), and `/status.memory` still `episodes: 1, verified: 1`: a refresh, never a
sibling row identity.

### Arm 3 — stranger (agent a153, task-4) — PASS

Stranger goal, same grant. Stamp **`silent`**, `candidates_checked: 0` (different goal
hash — no candidates). Trajectory `read` → `done`, status Done — and no mint: a task
with no landed patch fails the mint bar, demonstrating verified-only minting from the
other side. Store unchanged.

### Arm 4 — drift (agent a154, task-5) — PASS

Workspace byte-reset, then `\n# drift\n` appended to `calc.py` (sha `836c6e38…`).
Repeat goal. Stamp **`silent`**, **`candidates_checked: 1`** — the candidate was
examined and the fingerprint gate refused it: drifted bytes get silence, exactly the
strangers-get-silence contract. The task then legitimately completed verified on the
drifted workspace and minted a **second** episode `e140a91a…` under the drifted
fingerprint set — a different task identity, the predicted-in-runbook outcome, not a
surprise. Final `/status.memory`: `episodes: 2, verified: 2, contradicted: 0,
parse_errors: 0`.

## 4. The store, on disk

`acceptance/evidence-raw/episodes.jsonl` (sha `d61cee1a5f0b80c6…`, 3 rows — the full
event-sourced history): mint `70d968d1…` → refresh `70d968d1…` (arm 2's re-mint,
later `minted_at`) → mint `e140a91a…`. Load-time state: 2 episodes, both verified,
0 parse errors — matching `/status` and `GET /memory` exactly.

## 5. Shutdown

PID re-confirmed via `readlink /proc/302688/exe` before `kill`; process gone on
re-check; GPU back to the desktop residents. Raw artifacts (both journals, the store
file, `daemon.log`, final `/status` and `/memory` bodies) retained under
`.superpowers/sdd/2026-08-26-memory-organ/acceptance/evidence-raw/` (git-ignored,
local — paths are box-local, per the battery configs' own convention).

## 6. Verdict

All four pre-registered mechanism arms PASS on a real boot of the reviewed branch tip:
a verified task mints; a byte-identical repeat is retrieved, grant-gated, injected and
refreshes the same identity; a stranger goal and a one-byte-drifted workspace both get
stamped silence; contradiction correctly never fired, including on a live infra
failure after injection. The organ mechanism is accepted. Anything about whether
repeats *improve* remains the next slice's pre-registered question (spec §9).
