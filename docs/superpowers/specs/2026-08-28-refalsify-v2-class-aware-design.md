# Refalsify v2 — premise-verdict probe (class-aware, collapsed)

Closes the 2026-08-28 domain-of-validity erratum on
`2026-08-27-refalsify-on-exact-design.md` (§6): the v1 probe refutes
patch-class episodes by construction — the exact gate matches
pre-first-touch (defective) bytes, `run_evidence` is a post-condition
that passes only after the landed patches, and any clean nonzero exit
contradicts. A drift-free exact repeat therefore poisons its own true
lesson (demonstrated live through the registry seam, 0.08s, three
asserts).

Approved shape (Brice, 2026-08-28): class-aware verdicts, no schema
change — option (i) of the brainstorm. **Design-time discovery that
collapses it (reported here, not absorbed silently):** the mint bar
itself requires a successful patch — `memory/mint.rs::verifying_run`
returns `None` unless a `patch && !failed` step exists — so a patchless
task never mints and **every mintable episode is patch-class**. The
"invariant-class" arm of option (i) is unreachable today. v2 therefore
implements the patch-class semantics unconditionally, with no classifier
branch: a live-but-unreachable arm would be untestable dead surface.
This also sharpens the erratum: v1's exit-0-injects semantics were wrong
for every real episode, not for a subclass.

## 1. The premise model

An episode exists because a task found a defect (the cited pre-state),
landed patches, and verified the fix. What its stored verification can
attest about the state the exact gate just matched is exactly one thing:
whether the episode's **premise** — "the defect is present" — still
holds.

- Stored command fails on the matched state → the premise holds; the
  lesson is applicable.
- Stored command passes → the premise is gone (the goal is already
  satisfied); the lesson is not *false* — the world just does not need
  it right now.

Named limitation (accepted, documented): a verification that is
state-independent (checks nothing the patches touched) fails only when
the environment is broken; v2 reads that as premise-held and injects a
lesson into a world where the task will fail anyway. The injection is
noise, not damage, and the passive contradiction path still catches the
aftermath. Distinguishing that case needs recorded pre-state evidence —
option (ii), out of scope, not foreclosed.

**Named limitation, second (2026-08-28, found during this arc's
implementation, verified by its reviewer and by the controller against
`crates/bloomery-daemon/src/task/registry.rs:599`):** a correct
`premise_held` injection into a task that legitimately completes without
its own patch-and-verifying-run cycle is passively contradicted by the
pre-existing memory-organ design §5 rule (`organ_after_run`: a scored
outcome with no verifying run contradicts whatever was injected) — the
poisoning is indistinguishable from "the lesson was wrong." This is
pre-existing discipline, not a v2 defect: the rule predates this spec and
is unchanged by it. What v2 changes is which episodes get injected via
probing — `premise_held` now injects into cases v1 either never probed
(flag off) or contradicted outright — so the practical weight this
discipline carries shifts even though its text does not. A future slice
weighing it should start from the memory-organ design's §5, not from this
probe.

## 2. Verdicts

Unchanged arms: the coverage pre-check and demotion boundary
(`skipped_ungranted` injects), the oversize gate (never probed, stamp
`None`), and every `failed: true` / signal-sentinel / unparseable arm of
`classify_probe` (`inconclusive` injects — probe infrastructure never
costs a task its injection).

The two clean outcomes invert, for every episode:

| raw outcome | v1 (retired) | v2 |
|---|---|---|
| clean nonzero | contradict + silent, `failed` | **inject, stamp `premise_held`** — the failure confirms the matched premise |
| exit 0 | inject, `passed` | **silent, stamp `premise_gone`** — no injection, **no store mutation**; the next identical retrieval re-probes |

Consequences, stated plainly:

- **No probe ever contradicts under v2.** Only the passive path (task
  ran with the injection and failed) contradicts, as before refalsify
  existed. v1's early detection of uncited drift is given up entirely —
  the erratum's honest trade, now total because the invariant class is
  empty.
- The stamp spellings `passed` and `failed` retire from reachable
  probe verdicts; journal consumers keep parsing them (rows written by
  v1 builds exist). Reachable set after v2: `skipped_ungranted`,
  `inconclusive`, `premise_held`, `premise_gone`, and absent (`None`).
- `premise_gone` tasks proceed memory-silent exactly as a stranger
  would; a goal-already-satisfied task discovers that trivially.
- `[memory] refalsify` stays the single flag, default **off**. v2
  removes the poisoning hazard; whether the probe *pays* (one bounded
  subprocess per retrieval buys premise-freshness) stays the future
  battery's question (v1 spec §6, transferred).

## 3. Implementation shape

- `classify_probe` (task/registry.rs) stays the raw exit classifier
  (`"passed" | "failed" | "inconclusive"` off the pinned outcome
  constants) — parsing rules and the -1 signal sentinel untouched. The
  caller maps its two clean outcomes to the v2 action + spelling
  (`"failed"` → inject + `premise_held`; `"passed"` → silent +
  `premise_gone`); `"inconclusive"` passes through unchanged.
- No classifier on `landed_patches`. The mint-bar invariant that makes
  this total (every episode has landed patches) is stated in a comment
  citing `verifying_run`'s patch requirement — if a future mint bar
  admits patchless episodes, that comment and this spec are the tripwire
  for revisiting.
- `OrganDecision.refalsify` stays `Option<&'static str>`; changes are
  additive at every consumer.
- Records: v1 spec §2.3 and §6 gain dated pointers "superseded by
  refalsify v2"; the erratum gains a "closed by v2" line; CARRIED-DEBT
  updated at merge.

## 4. Testing

- **The pin, red-first**: the erratum's spike test returns permanently,
  inverted — drift-free exact repeat, `refalsify = true`, verification
  keyed to the cited file's goal state (`grep -q 'x = 2' a.py`) → stamp
  `premise_held`, injected (`memory_prompts == 1`), store row still
  `verified`/uncontradicted. RED on v1 (the recorded 2026-08-28 spike
  run is the red: v1 stamps `failed` and contradicts), GREEN with v2.
- **premise_gone**: a fixture whose stored verification passes on the
  matched pre-state — the existing `CANARY_SCRIPT` (`echo ran >
  probe-ran.txt`) is exactly that — asserts: exit 0 → `premise_gone`,
  silent (`memory_prompts == 0`), store row untouched, and a THIRD
  identical task re-probes (canary reappears; no memoized skip, no
  contradiction ever).
- **Existing suite expectations flip where they must** (the plan owns
  each): `a_passing_probe_injects_and_stamps_passed` becomes the
  premise_gone behavior; `a_failing_probe_contradicts_silences_and_stamps_failed`
  becomes premise_held/injected (its uncited `flag.txt` drift now reads
  as premise-held — correct under v2's model: the stored command fails,
  the lesson injects, and if the lesson is genuinely stale the passive
  path catches it). Flag-off identity, ungranted skip, demotion,
  timeout, signal, and oversize tests are untouched.
- **Mutation checks**: swap the two clean-outcome actions (inject ↔
  silent) — killed by both pins; drop the no-store-mutation property of
  `premise_gone` — killed by its store assert; make `premise_held`
  contradict — killed by the pin's store assert.
- Suite-wide: `cargo test --workspace` green; vulkan featured build
  LAST (clobber rule).

## 5. Out of scope

- Recorded pre-state evidence / expectation matching (option ii).
- Any mint-bar change (incl. admitting patchless episodes).
- The battery slice: re-registered against v2 as its own future
  pre-registration; no number from this arc's tests may appear in a
  capability sentence (claim discipline, unchanged).
- Any retrieval-semantics, render, or window-law change.
