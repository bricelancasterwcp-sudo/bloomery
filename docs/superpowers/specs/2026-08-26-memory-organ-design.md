# The memory organ — slice 1: exact-repeat episodic store with grant-gated injection (mechanism)

**Date:** 2026-08-26
**Status:** Approved in conversation (rulings: slice shape = organ first,
measure later — mechanism acceptance now, the repeat-exposure battery is the
NEXT slice with its own instrument and pre-registration; task identity =
goal-hash + cited-file fingerprints, two-stage exact; record shape = episodes
only, rendered verbatim, no model prose in the store; mint bar =
verified-only, the productive-run standard; envelope rule = orthogonal +
stamped, every frozen instrument runs memory-off. Sub-rulings presented and
accepted with the design: find steps contribute no citations; at most one
episode injected; the store is global per-daemon with the grant gate as the
capability boundary; falsification is passive-only; storage is event-sourced
JSONL with no new dependency; operator surface = `/status` field + `GET
/memory` + `DELETE /memory/{id}`, dark when disabled.)
**Lineage:** crucible's three-phase findings arc
(github.com/bricelancasterwcp-sudo/crucible, `docs/findings/`): ABLATIONS-A —
retrieval-only carries Δ_second +13.5 pp, ~96% of the full system's +14.0, at
zero sleep GPU; GATE-B verdict **GO_B** — store-only retrieval on a 14B ten
times the 1.5B's size lifts second exposures 0.780 → 0.905 (+12.5 pp vs the
derived 8.07 bar), pure prompt-side, no training, and the exact-only probe
(Δ_second +15.0 pp, novel 0.40 → 0.44 with strangers-get-silence) shows
exact-class content drives the gain; GATE-C verdict **NO-GO** —
symptom-conditioned transfer moved non-repeats +1.2 pp against a 7.64 bar at
99.4% precision and 35% reach: non-exact retrieval fails for want of
material, not matching. Bloomery lineage: the drift watch
(`2026-08-17-drift-watch-design.md`) as the organ pattern — advisory,
config-gated, journaled, live-accepted on mechanism; the turn-6 honesty spec
(`2026-08-23-flywheel6-honesty-design.md`) whose motivating finding — models
fabricate repair claims in bare prose — is why nothing in this store is ever
model-written; the task loop (`crates/bloomery-daemon/src/task/task_loop.rs`)
whose `render_prompt_from` is the injection seam.

## 1. What this builds and why

Bloomery buys capability with training turns: turn 5 cost $6.32, a 4-hour
S3 upload, and a rental pod. Crucible's closed program says that for any
task the appliance sees **twice**, a prompt-side episodic store recovers
most of that benefit for free, at any model size, surviving base swaps that
discard trained adapters. This slice builds that store as a daemon organ —
sibling to drift and swap — and proves the **mechanism** end to end on this
box: a verified task mints an episode; an exact repeat retrieves it, passes
the grant gate, and gets it injected; a stranger gets silence; a drifted
workspace gets silence; every outcome is stamped in the journal.

**Claim discipline.** Crucible's numbers license exactly one claim: exact
repeats improve under exact-class retrieval, on crucible's stream, on
crucible's models. They license nothing about bloomery's tasks, fixtures,
or models. This slice therefore claims only mechanism; the capability claim
("repeats improve on bloomery") belongs to the next slice — a
repeat-exposure battery, its own frozen instrument, its own
pre-registration, floors set only after baselines. No number from this
slice's acceptance may appear in a capability sentence. Phase-C's NO-GO
stands as a standing prohibition: no non-exact retrieval mode ships without
a corpus-scale argument and a new pre-registration.

The organ is **advisory and inert by default**: config-gated off, it never
gates admission, never touches `done_trust`, never executes anything, and
its total failure must be indistinguishable from memory-off (§7).

## 2. The episode record

An episode is minted at exactly one bar — the task ended `Done` AND landed
at least one successful `patch` AND a granted `run` command exited 0 after
the last successful `patch` — computed
from the task's own `TaskStepRecord`s (`step`/`verb`/`outcome`/`failed`/
`args`), the same journal-bytes standard the batteries' productive-run
endpoint uses. Refusals, tasks without run grants, unverified dones, and
every non-`Done` status mint nothing. Silence is the default; the store can
only ever contain what has execution evidence.

The record (one JSON object, the payload IS the record — no field lives
only in an index):

- `episode_id` — content hash over (`goal_hash`, the cited-file
  fingerprint set): the **task identity**, deliberately excluding the
  landed patches, so a repeat verified with a different solution refreshes
  the same row (last-writer-wins) instead of minting a sibling.
- `goal_hash` + `goal_text` — hash of the normalized goal (trimmed,
  internal whitespace runs collapsed to single spaces); the text kept
  verbatim for operator display (`GET /memory`).
- `cited_files` — every path the task `read` or `patch`ed, absolutized,
  each with the sha256 of its bytes **before the task's first touch** of
  that path, captured during execution; a file the task created carries the
  distinguished fingerprint `absent`. `find` steps contribute no citations
  (their evidence is directory state, not file bytes) — a recorded
  limitation: a repeat whose difference is only visible to `find` will
  still match.
- `landed_patches` — the successful patch steps' codec text **verbatim**,
  in step order. On an exact repeat the model can literally replay them.
- `run_evidence` — the verifying run's argv and exit status.
- `trajectory` — the verb sequence, for operator display only.
- `minted_by` — served model identity and envelope version at mint.
  Recorded for honesty; retrieval never filters on either (GATE-B: the
  store is model-agnostic).
- `status` — `verified` | `contradicted`, with `contradicted_by` citing
  the journal of the task that contradicted it (§5).
- `minted_at` — epoch ms, stamped at append (a row property, per the
  journal precedent: never pre-register byte-identity of the store file).

Nothing in the record is model prose. The `done` summary text is
deliberately excluded — the flywheel5 battery §6.6 showed exactly that text
fabricating repairs.

## 3. Retrieval: two-stage exact match, then the grant gate

At task start (memory enabled), before step 1:

1. **Candidates** — episodes whose `goal_hash` equals the incoming goal's.
2. **Fingerprint gate** — for each candidate, hash each cited file in the
   current workspace (paths absolutized against the task's `cwd`, the same
   absolutize step the executors use). Every fingerprint must match —
   `absent` must match a file that does not exist. Any mismatch, or any
   unreadable file, disqualifies the candidate (unreadable = mismatch,
   never an error).
3. **Grant gate** — every cited path must fall inside the requesting
   grant's read roots. Memory must never show an agent bytes its own grant
   could not have read. The store is global per-daemon (agents are
   ephemeral); this gate, not per-agent partitioning, is the capability
   boundary.
4. **Status gate** — survivors must be `verified`.

At most **one** episode is injected: the most recently verified survivor.
Zero survivors — including the memory-off case — is silence.

## 4. Injection and the envelope rule

`render_prompt_from` (task_loop.rs:343) currently renders
`{goal}\n\n{grant_section}{verb_card}\n\n{transcript}`. The memory block
becomes an optional section rendered **immediately after the goal block and
before the grant section**, and it renders to the **empty string** when the
organ is off or silent — memory-off output stays byte-identical to today,
provable against the existing envelope goldens. When an episode is
injected, the block is a delimited, deterministically rendered "verified
prior attempt": the same goal was completed before against byte-identical
starting files, these patches landed (quoted verbatim), this granted
command exited 0 afterward. No advice, no paraphrase — quoted evidence
only.

**Envelope rule (as ruled):** the memory section is orthogonal to the
envelope lattice — it renders identically under every `EnvelopeLens` and no
envelope version v6 is minted for it. Lens-travels-with-verdict is
satisfied by **record**: every task journals a memory stamp — mode
(off/on), what was injected (`episode_id` + the matched fingerprints) or
`silent` with `candidates_checked` — as an additive `Event` variant (old
journals replay unchanged). Minting and contradiction journal their own
rows, so the task → store evidence trail is walkable in both directions.
**Every frozen instrument — G4/G5 batteries, drift probes, swap cover —
runs memory-off**; the future memory battery is its own instrument and
pre-registers its own memory-on lens.

## 5. Falsification: passive only

> **Amended 2026-08-27:** active refalsification exists now, as a
> task-scoped probe under the incoming task's grant — see
> `2026-08-27-refalsify-on-exact-design.md`. Daemon-spontaneous execution
> stays banned; everything else in this section stands.

The organ never executes anything, so falsification never re-runs a cited
command. Two guards carry it:

- **The fingerprint gate itself** — most rot is workspace drift, and
  drifted bytes already produce silence at retrieval, for free.
- **Passive contradiction** — if a task that *received* an injected
  episode then fails its own verification (no productive run after its
  last patch, or a non-`Done` end), the episode's status becomes
  `contradicted`, citing that task's journal, and it is never injected
  again. A repeat that succeeds refreshes `verified` (and re-mints the
  same `episode_id` with a fresh `minted_at`). Contradiction is computed
  from the same `TaskStepRecord` evidence as minting — the organ only ever
  reads outcomes, never creates them.

Crucible's active re-falsification (re-executing a lesson's cited tests
before it keeps its place) is named as a possible later, operator-triggered
slice; spontaneous daemon-initiated execution is against the house rules
and stays out.

## 6. Storage, operator surface, retention

**Store:** `<data_dir>/memory/episodes.jsonl`, append-only, event-sourced:
every row is a complete record (payload-is-the-record); a status change
appends a new full row for the same `episode_id`; replay is
last-writer-wins per id. The in-memory index (by `goal_hash`) is rebuilt at
load and is never the source of truth. No new dependency — no SQLite; this
is the journal idiom applied to the store. A corrupt line is **counted and
surfaced** (`parse_errors` in `/status`), never fatal, never silently
dropped.

**Config:** a `[memory]` section — `enabled` (default `false`),
`max_episodes` (retention cap on distinct ids; eviction order is
contradicted-oldest-first, then verified-oldest-first). The
single-injection cap is a constant, not config.

**Operator surface:** `/status` (api_native.rs:628) gains
`memory: {enabled, episodes, verified, contradicted, parse_errors}`.
`GET /memory` lists episodes (id, goal text, cited paths, status,
minted_at, minted_by). `DELETE /memory/{id}` purges one id — an appended
tombstone row; a later verified completion may legitimately re-mint the
id (the operator deleted a row, not banned an identity). The operator's
eviction right is part of the organ's trust story. Both routes are dark — `501`, before the body is parsed — when
`enabled` is false, per the `tasks_enabled` pattern.

**Concurrency:** the store lives behind a single mutex and has no
background writer; whether it shares the pager's lock or carries its own
is the plan's choice.

## 7. Error handling

The organ being broken can only ever produce memory-off behavior — never a
wrong injection, never a failed task:

- Mint-time store IO failure: journal a warning row; the task's own result
  is unaffected.
- Retrieval-time hashing failure on any cited file: that candidate is a
  mismatch (silence), not an error.
- Store unreadable at boot: organ reports itself disabled-with-reason in
  `/status`; tasks run memory-off.
- A `DELETE` for an unknown id: `404`, no store mutation.

## 8. Testing and acceptance

TDD throughout; the whole loop exercises GPU-free against `FakeSubstrate`
with scripted `<action>` turns, per the task-loop test pattern. Binding
tests, at minimum:

- **Mint bar** and each negative: refusal task, no run grant, run after
  patch fails, run before the last patch only, non-`Done` statuses.
- **Fingerprint capture**: pre-first-touch semantics (read-then-patch
  hashes once, at first touch), `absent` for created files.
- **Retrieval**: exact hit injects; one changed byte in one cited file is
  silent; `absent`-vs-exists is silent; grant not covering one cited path
  is silent; contradicted is silent; two survivors → most recently
  verified wins.
- **Render**: memory-off and memory-silent prompts are byte-identical to
  the current renderer's output (golden), under every envelope version.
- **Contradiction**: injected-then-failed marks `contradicted` and the
  next repeat is silent; injected-then-verified refreshes.
- **Store**: replay last-writer-wins, corrupt-line counting, tombstones,
  retention eviction order.
- **Surface**: `/status` field, `GET /memory`, `DELETE`, both routes dark
  when disabled.
- **Journal**: stamp rows for on/injected, on/silent, mint, contradiction;
  old journals replay unchanged.

The three predicates that carry the organ — the mint bar, the fingerprint
compare, the grant-subset check — get mutation-checked (deliberately broken
implementations must fail the suite).

**Live acceptance (HUMAN-GATED, real boot, resident model):** in a scratch
workspace, (1) run a real task to a verified `Done` — store shows one
verified episode, journal shows the mint; (2) reset the workspace to its
pre-task bytes, resubmit the same goal — the stamp shows the injection and
the task completes; (3) a stranger goal — silent; (4) hand-drift one cited
file, resubmit — silent. Mechanism claims only; the evidence doc records
stamps and store rows, and no capability sentence.

## 9. Out of scope (named so their absence is a decision)

The measured repeat battery (next slice — own instrument, own prereg,
floors after baselines); refusal memory (remembering correct refusals);
negative episodes (failed attempts as warnings); `find`-citation
fingerprints; active re-falsification; any non-exact retrieval (Phase-C
NO-GO stands until a corpus-scale argument exists); retention policy beyond
the count cap; cross-daemon sync; memory for `/v1` chat traffic (tasks
only).

## 10. Delegated to the plan

Pinned semantics whose exact shapes the implementation plan chooses: the
`Event` variant layouts for stamp/mint/contradiction; where the
pre-first-touch hash is captured (task_loop vs the exec layer — the
semantic in §2 is binding, the seam is the plan's); the memory block's
delimiter text and worked rendering (spec rule: quoted evidence only, no
advice prose); the eviction tombstone row shape.
