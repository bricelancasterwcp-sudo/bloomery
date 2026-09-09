# hermes-on-bloomery dogfood, 25-tool default set — **PARTIAL** (2 of 3 pre-registered sessions PASS; the third refused by residency, recovered post-hoc via `DELETE /agents/{id}`)

**Date:** 2026-09-07. Pre-registration: [`2026-09-07-hermes-dogfood-25-tools-prereg.md`](2026-09-07-hermes-dogfood-25-tools-prereg.md)
(written before boot; nothing in it changed after the first number was read).
Adapter per-request log: [`2026-09-07-hermes-dogfood-25-tools-adapter.log`](2026-09-07-hermes-dogfood-25-tools-adapter.log).

## Lens

- bloomery master `3866da0`, daemon rebuilt `--release --features llama,vulkan` this session
  (a plain `--release` build produces a 1.2 MB binary that refuses to serve — "built without the
  `llama` feature" — the featured binary is 47.5 MB). Includes the 08-31 `/v1` honesty fix,
  `DELETE /agents/{id}` (#22) and `max_placeable_tokens` (#23), none of which the 08-31 runs had.
- adapter `adapters/openai-tools/` at the same commit (132/132 unit tests OK before the run).
- model `qwen36-reap48-ours` **untrained** Q4_K_M, sha256 `90e2181e…` == the daemon's
  reported digest (served identity verified, not just liveness). Same lens as the 08-31
  measurement arc, so the numbers below are comparable to it.
- daemon config byte-identical to the 08-31 `measure-bloomery.toml` except a fresh
  `data_dir` on `/mnt/extra`; adapter config byte-identical (`window_cap` 30000, `max_tokens` 768).
- hermes: the live install, driven under a **fresh scratch `HERMES_HOME`** (config = the 08-31
  scratch profile: provider ollama, base_url → adapter). Toolset = hermes-cli default, **no `-t`
  flag**, non-interactive `-Q -q`, tools executing, no `--safe-mode`, no `--yolo`.
  Live gateway PID 1169016 (41 d uptime) and `~/.hermes/config.yaml` (mtime 2026-07-26) untouched.
- sampler: substrate greedy, pinned by the daemon.
- task: create `/tmp/dogfood-20260907/run<N>/` with `a.txt`/`b.txt`/`c.txt` = alpha/beta/gamma,
  list, report — the 08-31 task with a per-run directory. Three fresh sessions against ONE
  adapter process.

**Why this run exists:** the 08-31 PASS (0 resets, deltas 32/212/54) was measured with a
4-tool set (`-t file`). The 25-tool default set had only ever been driven BEFORE the
streaming/retry/identity fixes, and that run was PARTIAL. This is the first time the fixed
adapter met hermes's real default toolset.

## Result

| run | wall | requests | agent | resets | `prompt_tokens` | tool calls | files on disk |
|---|---|---|---|---|---|---|---|
| 1 | 19 s (incl. ModelLoaded) | 3 | a1 | 0 | **18,957**, 293, 171 | terminal, write_file ×3, terminal | 3/3 exact |
| 2 | 12 s | 3 | a2 | 0 | 18,957, 293, 171 | terminal, write_file ×3, terminal | 3/3 exact |
| 3 | 2 s | 1 | a3 | 0 | `None` | — | 0/3 — **HTTP 409** |
| 4 (post-hoc, after `DELETE` a1–a3) | 11 s | 3 | a4 | 0 | 18,957, 293, 171 | same | 3/3 exact |

Per-request outcomes: runs 1, 2, 4 all `ok`; run 3 `bloomery_error_409` with `prompt_tokens: null`
(unmeasured is `None`, not 0).

### Endpoints (pre-registered)

- **E1 functional — FAIL, 2/3.** Runs 1 and 2 produced all three files with exact contents.
  Run 3 wrote nothing: hermes displayed bloomery's refusal unaltered —
  `HTTP 409: the model could not be made resident: needed 1217134592 B, free 545745056 B, reclaimable 0 B.`
- **E2 prefill-once — PASS 2/2 measurable sessions.** One 18,957-token preamble, then 293 and 171;
  one agent, zero resets, per session. Session 3 has no measurable request.
- **E3 preamble — 18,957 tokens**, fits under `window_cap` 30,000. This is ~4,900 more than the
  08-31 estimate for the 25-tool set (14,077, taken from a request dump), i.e. hermes's current
  system prompt under a fresh profile is larger than the dumped one. Recorded, not explained.
- **E4 parse — PASS on every served turn.** Ten tool calls across runs 1–2 (fifteen with run 4),
  including a 4-call turn (`terminal` mkdir + three `write_file`, 267 completion tokens), every one
  well-formed and executed by hermes. Still a handful of trajectories, not a rate.
- **E5 daemon health — PASS.** `/status` answered in 0.2 ms after all runs; journal shows
  `InferStarted` 9 == `InferCompleted` 9, `AgentCreated` 4, `AgentRemoved` 3, one `Refusal`.
  No wedge (the pager-lock-across-infer class did not fire).
- **E6 retries — 0.** hermes sent no byte-identical retry; the replay path was not exercised.

**Verdict: PARTIAL** (E1 fails at 2/3; E2/E4/E5 pass). The point estimate stands; no rerun.

## The finding: sessions leak agents, and the tier's turnover rule never fires for a one-shot client

Read from `/status` at the moment of the refusal: a1 and a2 were both `resident`, each holding
1,217,134,592 B (kv 614,400,000 B for the 30,000-token cap + ctx overhead 602,734,592 B),
`resident_kv_bytes` 2,434,269,184; a3 `fresh`. The refusal arithmetic in the journal:

```
budget 15,809,380,352 − overhead 1,073,741,824 − loaded 11,755,624,288 − resident 2,434,269,184
= free 545,745,056 B; needed 1,217,134,592 B; reclaimable 0 B; largest placeable window 0 tokens
```

So this tier holds **two** 30k-window sessions (the 08-31 "one context" was at a larger window
and a smaller measured budget), and the third is refused. Why nothing was reclaimable, from the code:

1. `plan_residency` (`crates/bloomery-core/src/scheduler.rs`) evicts only residents with priority
   **strictly lower** than the requester. Every adapter agent is created at `default_priority` 100,
   so no adapter agent can ever evict another. `reclaimable` is the sum over that (empty) set: 0.
2. The equal-priority path is `try_time_share` (`pager/paging.rs`): it evicts the LRU idle resident
   only once the **requester** has been waiting a full `time_share_quantum_secs` (30 s), keyed by the
   requesting agent id. a3's first and only request arrived at `waited_ms` 0 → refused. hermes does
   not retry a 409, and a `-q` session exits; nobody ever waits the quantum.
3. The adapter has no session-end signal (hermes just exits), and it suspends agents only on a
   history rewrite, so a1 and a2 stayed resident and idle forever.

Net: **after N successful hermes conversations, where N is how many windows the tier holds (two
here), every further conversation is refused until an operator deletes agents by hand.** This is the
CARRIED-DEBT "agents accumulate" item observed for the first time as a user-facing failure rather
than as an `a7` in a status listing.

**Recovery path works (post-hoc probe, not a rerun of the endpoint):** `DELETE /agents/a1`, `a2`, `a3`
→ 204 each, `resident_kv_bytes` 0, and a fourth session then passed with numbers identical to
runs 1–2. That is the first live exercise of #22's endpoint.

## What this means for "ready for use with hermes"

- The adapter + daemon + untrained model serve hermes's real 25-tool default set correctly, with
  prefill-once holding at 18,957 tokens and every tool call parsing, at ~12 s per three-turn task.
- It is **not** a daily driver until agent turnover is owned by someone: candidate fixes, none built —
  (a) the adapter reaps idle sessions (a TTL, then `DELETE`), (b) the adapter creates agents below
  `default_priority` so `plan_residency` can evict its own idle agents, (c) bloomery gains an
  idle-eviction rule that does not require the requester to wait the quantum. (a) needs no Rust
  and is the smallest; (b) is a one-line config choice but makes every adapter agent evictable by
  any native-API agent; (c) is a pager design change. Ruling owed.
- Tier fact update: **two** 30k contexts fit beside the 10.95 GiB weights on this box today
  (free VRAM read 14.72 GiB at boot vs 13.10 GiB on 08-31 — the budget is a point-in-time probe).

## Not done / limits

- Three pre-registered runs plus one probe: parse quality is still a handful of trajectories.
- The preamble-size growth (14,077 → 18,957) is measured, not explained.
- E6 exercised nothing: no retry occurred, so the 08-31 replay fix has no new evidence here.
- The `-q` client never waits the time-share quantum; whether an interactive hermes session that
  retries after 30 s would be admitted via LRU eviction was not tested.

## Ops

- Everything ran OS-detached (`setsid nohup`), killed by PID from pid files, never `pkill -f`.
  Daemon and adapter were shut down after the run; GPU back to 663 MiB.
- Data dir `/mnt/extra/bloomery-dogfood-20260907` (248 KB: journal only, no images were ever saved
  because nothing was ever evicted) left in place as the journal of record.
- Scratch `HERMES_HOME` lived in the session scratchpad and is not retained; its `config.yaml`
  was the 08-31 profile verbatim (reproduced in the pre-registration).

---

# Addendum: the idle-reap follow-up run — pre-registration

Recorded 2026-09-07, BEFORE the daemon was booted for this run, after Brice
ruled option (a): the adapter owns agent turnover (`adapters/openai-tools/openai_tools/reap.py`,
branch `adapter-idle-reap`; 152 adapter tests green, 13/13 mutants killed).

**Lens changes vs the run above, and nothing else:** adapter code at the branch head
(idle TTL 900 s via the default, so the TTL sweep cannot fire inside a ~40 s run — only
the residency reap can); fresh `data_dir` `/mnt/extra/bloomery-dogfood-20260907-reap`.
Same daemon binary, model, daemon config, adapter config values, scratch `HERMES_HOME`,
25-tool default set, prompt shape, three back-to-back `-q` sessions. Run directories
`/tmp/dogfood-20260907-reap/run<N>/`.

**Endpoints (decided by the point estimate; no rerun):**

- **E1 functional — 3/3** files exact (was 2/3).
- **E2 prefill-once — 3/3** sessions: one preamble ≥ 10,000 tokens, every other request
  < 1,000, one agent per session, zero resets.
- **E4 parse —** every served turn `ok`; the run-3 first attempt MAY log `bloomery_error_409`
  (that is the reap trigger, not a failure) and must then be followed by `ok`.
- **E5 daemon health —** `/status` answers within 5 s after all runs; every `InferStarted`
  paired with `InferCompleted`.
- **E7 reap (new) —** the adapter log contains exactly ONE `session_reaped` event, with
  `reason: "residency"`, naming run 1's agent (`a1`, the LRU), emitted during run 3; zero
  `delete_failed`; the journal shows one `AgentRemoved` before run 3's `InferCompleted`.
- **E8 residency (new) —** after all runs `/status` lists at most 2 agents, none `fresh`.

**Verdict rule:** PASS = E1 ∧ E2 ∧ E4 ∧ E5 ∧ E7 ∧ E8. Anything else PARTIAL, endpoint named.
Kill criteria unchanged from the pre-registration above.

## Run B result — **PARTIAL** (E7 fails on a logging defect; the reap itself worked)

Adapter log: [`2026-09-07-hermes-dogfood-25-tools-reap-runB-adapter.log`](2026-09-07-hermes-dogfood-25-tools-reap-runB-adapter.log).

| run | wall | requests | agent | `prompt_tokens` | outcomes | files |
|---|---|---|---|---|---|---|
| 1 | 13 s | 3 | a1 | 18,959, 305, 171 | ok ×3 | 3/3 exact |
| 2 | 11 s | 3 | a2 | 18,959, 305, 171 | ok ×3 | 3/3 exact |
| 3 | 12 s | 4 | a3 | `None`, 18,959, 305, 171 | **409, then ok ×3** | 3/3 exact |

- **E1 3/3 PASS. E2 3/3 PASS** (one agent, zero resets per session). **E4 PASS** — run 3's first
  line is the pre-registered 409 trigger, followed by `ok`. **E5 PASS** — `InferStarted` 9 ==
  `InferCompleted` 9, `/status` answered. **E8 PASS** — `/status` after: a2, a3 `resident`,
  nothing `fresh`. The journal shows `Refusal a3` → `AgentRemoved a1` ("operator requested
  removal via DELETE /agents/{id}") → `InferCompleted a3` in that order: **the residency reap
  fired exactly as designed, 12 s for the whole third conversation.**
- **E7 FAIL — measured 0 `session_reaped` events, not 1.** Not a reaping defect: the DELETE is
  in the daemon's journal. A logging defect: `server.py` named its logger from `__name__`, which
  under `python -m openai_tools.server` is `__main__`, while `reap.py` logs to the README's
  promised `openai_tools.server` — a name that, in production only, had no handler, so an
  INFO event was silently dropped (a WARNING `delete_failed` would have reached Python's
  last-resort handler; the success event did not). Every test imported the module under its
  package name, where the two names coincide, which is why 153 green tests never saw it.
  **A value that looks like a measurement but is not** — the run's instrument, not its subject.

Fixed on the branch (fixed logger name; `EntrypointTest` executes `server.py` under the name
`__main__` exactly as `-m` does, with `serve_forever` stubbed, and asserts the reap and request
loggers are one object; the mutant that restores `__name__` is killed). A second defect found
while stabilising the suite: `last_used` was stamped AFTER the response was written, so a reaper
acting the instant a client got its answer could read a stale stamp or one taken from a clock
that had already advanced — one flaky failure in six runs. It is now stamped at request start,
under the lock; 8/8 consecutive green; 14/14 mutants killed; 153 tests.

Run B's point estimate stands as PARTIAL. It is not re-rolled: run C below is a fresh run under
a new lens (the fixed adapter), pre-registered before boot.

## Run C — pre-registration (written before boot)

Identical to run B's pre-registration in every respect except: adapter at the branch head after
the two fixes above; fresh `data_dir` `/mnt/extra/bloomery-dogfood-20260907-reapC`; run
directories `/tmp/dogfood-20260907-reapC/run<N>/`. Endpoints E1, E2, E4, E5, E7, E8 and the
verdict rule are unchanged and copied by reference, not restated, so they cannot drift.

## Run C result — **PARTIAL**: the reap passes its own endpoint; the lens moved under the run

Adapter log: [`2026-09-07-hermes-dogfood-25-tools-reap-runC-adapter.log`](2026-09-07-hermes-dogfood-25-tools-reap-runC-adapter.log).

| run | wall | adapter requests | agents used | resets | files |
|---|---|---|---|---|---|
| 1 | 24 s | 6 | a1 → a3 → a4 | 2 | 3/3 exact |
| 2 | 15 s | 4 | a5 → a7 | 1 | 3/3 exact |
| 3 | 15 s | 5 (first one 409) | a8 → a10 | 1 | 3/3 exact |

- **E1 3/3 PASS. E4 PASS** (every served turn `ok`; run 3's first line is the 409 trigger).
  **E5 PASS** (`InferStarted` 14 == `InferCompleted` 14; `/status` answered).
- **E7 PASS — exactly one `session_reaped` event, `reason: "residency"`, zero `delete_failed`,
  and the journal shows `Refusal a8` → `AgentRemoved a4` → the retried inference completing.**
  The logging fix is confirmed live: the event now reaches the same stream as the request lines.
  The victim was a4, not a1: a1 had already been suspended by a reset (below), so a4 was the
  least-recently-used *resident* session, which is the right victim.
- **E2 FAIL (0/3) and E8 FAIL** (nine agents listed, several `suspended`/`fresh`): every session
  shows one or two extra requests of 1,278 bytes / 315 prompt tokens with a 768-token answer
  (`finish_reason: length`), each classified a history rewrite → reset → new agent → the whole
  17,493-token preamble prefilled AGAIN (17,970 on run 1's a4). Net: two full prefills per
  conversation instead of one, and agent churn a1…a10.

**Cause — the lens changed between runs B and C, outside this run's control.** hermes updated
itself on disk at 09:18:12 (every file under `~/.hermes/hermes-agent/agent/` carries that mtime;
run B started 09:17:10, run C 09:28:19), and the new version issues an **auxiliary
title-generation LLM call** through the same `base_url` (`agent.log`: "Auxiliary title_generation:
using custom (qwen36-reap48-ours) at http://127.0.0.1:8011/v1/" at 09:28:22, 09:28:45, 09:29:00 —
once per session; `title_generator.py` sends `[system: <title prompt>, user: <first user
message>]`). The adapter keys sessions by the SHA-256 of the **first user message** when no
`X-Session-Id` is sent, so the title call and the conversation collide onto one session and take
turns rewriting each other's history. This is the README's documented "session identity is a
heuristic" limitation, measured for the first time with a real client. Two consequences worth
recording: (1) the title call's answer ran to 768 tokens = the adapter's default `max_tokens`,
although `title_generator.py` passes `max_tokens=64` — the value evidently did not arrive in a
key the adapter reads (unverified: no wire capture in this run); (2) the run-B preamble of
18,959 tokens became 17,493 under the new hermes — the system prompt changed with the update,
which is why E3-type numbers are point-in-time and carry their hermes version.

Verdict stands as measured. Not re-rolled: the collision is real, orthogonal to the reap, and
would recur on every run under this hermes version.

**Ruling owed (next slice, not this one):** give auxiliary calls their own identity. Options:
(a) key sessions on the system prompt AND the first user message, so a differently-prompted
call gets its own agent (then reaped by TTL/residency like any other); (b) treat requests that
carry no `tools` as ephemeral — create, infer, `DELETE` in one request — the cheapest for a
64-token title; (c) ask hermes to send `X-Session-Id`. (b) also stops a title call from ever
holding a 30k-token window. Separately, check what `max_tokens` key the auxiliary client sends.

**Ops note:** the daily gateway (PID 1169016) is still running the pre-update code in memory;
its next restart picks up the 09:18 on-disk version, title calls included.
