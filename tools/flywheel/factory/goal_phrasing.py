"""Shared goal-phrasing skeleton assemblers (task 6a's third follow-on
fix). Every template family used to hand-write exactly ONE fixed goal
sentence structure. Even after the second follow-on fixed identifier/
numeric entropy, the SAME family measured >=0.8 token-set Jaccard
against its own prior draws (py_inverted_boolean: 0.88 between two
draws with entirely different noun/threshold/values) -- because a single
fixed skeleton's CONNECTIVE/FRAMING words ("requires both conditions
instead of either one (or vice versa) -- ... returns ... instead of ...
Fix ... so ... is correct") outnumber the few identifier tokens that
actually vary, and the mandatory closing instruction (6-13 more fixed
tokens) compounds it. At full scale this showed up as 99% of draws
rejected on `goal_near_duplicate` against the frozen, single-skeleton-era
`codec-tasks-v2-mixed` gate.

The fix: each family now offers >= 4 skeletons -- genuinely different
sentence structures and framing/connective vocabulary, chosen per draw
via `rng.choice` (continuing the family's own rng stream; determinism,
rule 3 unaffected). The CONTENT clauses a family computes (what's wrong,
the concrete evidence, what to change) stay the same text regardless of
which skeleton is chosen -- only how they are introduced and stitched
together differs. This is deliberate, not a shortcut: the token overlap
that actually matters is between DIFFERENT draws (different identifiers,
almost always different skeletons too), where the shared vocabulary is
now just the closing instruction plus a handful of connective words --
comfortably under 0.8 Jaccard for typical goal lengths. Two draws that
happen to land on the SAME skeleton (~1-in-N chance) may still collide
near-fully with each other or with the gate's frozen (single-skeleton)
text; that is an expected MINORITY the gate-aware rejection sampler
(`gate_sampling.py`) already absorbs by redrawing.

Four shapes cover every family in this factory:

- `patch_skeletons` -- every `expect="patch"` family (python and
  plaintext lenses alike): states what's wrong, evidence, and the fix.
- `defect_absent_skeletons` -- refusal families where the file is
  genuinely correct and the goal claims a plausible-but-false defect
  (`task.REFUSAL_QUOTED_RE`'s plausibility rule: the family's own
  `claim` string must carry a backtick-quoted identifier/value that is a
  REAL substring of the generated file -- unaffected by which skeleton
  wraps it, since `claim`'s own text is skeleton-invariant).
- `missing_target_skeletons` -- refusal families where the named target
  does not exist at all.
- `symptom_mismatch_skeletons` -- refusal families where the file really
  IS broken but the reported symptom is a different, absent defect. Its
  skeletons all frame the claim as a REPORTED observation (a field
  report, a page, a handoff note) rather than as a question or a
  verification request: the defect-absent framings ("is it true that
  ...?", "please verify ...") already invite a check, whereas this
  family's whole difficulty is that a confident-sounding report is wrong
  about a file that nevertheless has something wrong with it.
"""

from __future__ import annotations

import random


def patch_skeletons(
    rng: random.Random,
    target: str,
    subject: str,
    problem: str,
    evidence: str,
    fix_target: str,
    fix_goal: str,
    instruction: str,
) -> str:
    """`subject` names WHAT is buggy without the target prefix (e.g.
    `"highest_temperature()"` or `"the snapshot_interval_min setting"`).
    `problem` is a subject-less predicate clause describing the bad
    behavior. `evidence` is the concrete before/after (or consequence)
    clause. `fix_target` names what to change and where; `fix_goal`
    states the desired end state, always phrased as a standalone clause
    (e.g. `"it keeps the highest temperature"` or `"the connector is
    correct"`) so every skeleton can splice it in after a dash or colon
    without needing a specific connecting verb.

    The four skeletons deliberately use almost NO overlapping glue
    vocabulary with each other (measured: a worst-case pair -- two draws
    whose `problem`/`evidence`/`fix_goal` happen to coincide, e.g. a
    small bounded value space recurring by chance, differing only in
    `target` -- still lands under 0.8 Jaccard specifically BECAUSE each
    skeleton's own framing words are unique to it; a naive design where
    skeletons differed by only one verb swap ("Fix" vs "Correct") did
    NOT clear this bar, since `problem`+`evidence`+`fix_goal` dominate
    the token count when they coincide)."""
    skeletons = (
        f"{target}'s {subject} {problem} -- {evidence}. Fix {fix_target} so {fix_goal}. {instruction}",
        f"Bug ticket for {target}: {subject} {problem}. Evidence: {evidence}. Resolution: update "
        f"{fix_target} -- {fix_goal}. {instruction}",
        f"A reviewer flagged {target} because {subject} {problem}, shown by {evidence}. Needed change: "
        f"{fix_target} -- {fix_goal}. {instruction}",
        f"Please correct {fix_target}: {subject} currently {problem}, and {evidence} proves it. Once "
        f"corrected, {fix_goal}. {instruction}",
    )
    return rng.choice(skeletons)


def find_skeletons(
    rng: random.Random,
    subject: str,
    problem: str,
    evidence: str,
    fix_goal: str,
    instruction: str,
) -> str:
    """`patch_skeletons`'s find-shaped counterpart (turn-3 design doc §2:
    "the goal names the symptom, never the file"). Same content clauses,
    minus every one that carried a filename: there is no `target` and no
    `fix_target` argument, because a goal that named either would hand the
    model the answer and turn the opening `find` into a decorative step
    (`task.validate_task`'s find branch refuses such a goal outright).

    Each skeleton instead frames the missing location explicitly -- "find
    it", "locate the module", "search the tree" -- so the trajectory's
    first move reads as the obvious response to the goal rather than an
    unmotivated flourish."""
    skeletons = (
        f"Somewhere in this workspace {subject} {problem} -- {evidence}. Track down whichever file "
        f"owns it and change it so {fix_goal}. {instruction}",
        f"Bug report, no file attached: {subject} {problem}. Evidence: {evidence}. Locate the "
        f"module it lives in, then make sure {fix_goal}. {instruction}",
        f"A support escalation says {subject} {problem}, and {evidence} backs that up. Search the "
        f"tree for whatever defines it; once corrected, {fix_goal}. {instruction}",
        f"Nobody on the team remembers where {subject} lives, but it {problem}: {evidence}. Find "
        f"it first -- afterwards {fix_goal}. {instruction}",
    )
    return rng.choice(skeletons)


def defect_absent_skeletons(rng: random.Random, target: str, claim: str, instruction: str) -> str:
    """`claim` is the full plausible-but-false defect clause, ALREADY
    carrying >= 1 backtick-quoted identifier/value that is real in the
    generated file (the plausibility rule) -- skeleton-invariant by
    design, since the backtick anchor must survive regardless of which
    skeleton wraps it. Skeletons vary only the reporter/framing words."""
    skeletons = (
        f"Someone on the team suspects that {claim}. {instruction}",
        f"Ticket filed against {target}: is it true that {claim}? {instruction}",
        f"A teammate double-checking {target} asks -- {claim}. {instruction}",
        f"Before touching {target}, please verify: {claim}. {instruction}",
    )
    return rng.choice(skeletons)


def symptom_mismatch_skeletons(rng: random.Random, target: str, claim: str, instruction: str) -> str:
    """`claim` is the full reported-but-absent symptom clause, ALREADY
    carrying >= 1 backtick-quoted identifier/value that is real in the
    generated file (the plausibility rule applies unchanged: the file has
    a REAL defect, just not this one, so a goal quoting nothing real
    would again teach "weird goal -> refuse"). Every skeleton names
    `target` itself, so the structural "goal contains the target
    filename" rule holds no matter what the family's claim says."""
    skeletons = (
        f"Field report on {target}: {claim}. {instruction}",
        f"An on-call engineer paged about {target} -- {claim}. {instruction}",
        f"Handoff note, {target}: {claim}. {instruction}",
        f"During review of {target} somebody wrote up this symptom -- {claim}. {instruction}",
    )
    return rng.choice(skeletons)


def missing_target_skeletons(rng: random.Random, missing_target: str, claim: str, instruction: str) -> str:
    """`claim` is the full false-premise clause naming `missing_target`
    (a file that does not exist among the fixture's files) and the
    symptom it supposedly has. Skeletons vary the reporter/framing
    words; `missing_target` is repeated in most skeletons on purpose
    (structural rule: the target filename must appear in the goal)."""
    skeletons = (
        f"{claim} -- can you check {missing_target} and fix it if that's really the bug? {instruction}",
        f"Ticket: {claim}. Please verify against {missing_target} before making any change. {instruction}",
        f"A user reported that {claim[0].lower()}{claim[1:]}. Take a look at {missing_target} and correct "
        f"it only if the report holds up. {instruction}",
        f"Before editing anything, check {missing_target} -- reportedly {claim[0].lower()}{claim[1:]}. "
        f"{instruction}",
    )
    return rng.choice(skeletons)
