"""`codec-tasks-v5-mixed`'s disjointness from ALL FOUR older gate sets
(turn-6 spec §4.3), split out of `test_contamination_g5_v4.py`'s lineage
for the same 400-line house cap that split every prior set's module.

The mechanism is `V4MixedDisjointTest`'s, applied a fifth time and against
four gates instead of three: v5-mixed is factory-authored, not a training
corpus, so the guard's normal direction ("corpus vs gate") inverts --
export every v5-mixed FILE as a pseudo-corpus row and run the SAME
`check_corpus` comparator, once per older gate set.

Turn 6 adds one thing this module is the right home for: every v5
`refusal_reason` is the FULL ideal declared `done` (spec §4.2), assembled
by the factory's `done_v5`. The Rust freeze suite checks the frozen bytes'
grammar and evidence grounding independently; THIS module proves the
stored text is byte-identical to what the LIVE `done_v5` assembler
produces from its own decomposition -- so a factory-side edit to the
assembler's shape that the Rust transcription cannot see fails here, and
turn 7's corpus renderer is guaranteed the same contract the fixtures
carry.
"""

import re
import tomllib
import unittest
from pathlib import Path

from tools.flywheel.factory import contamination, goal_phrasing
from tools.flywheel.factory.task import done_v5

REPO_ROOT = Path(__file__).resolve().parents[3]
FIXTURE_DIR = REPO_ROOT / "crates" / "bloomery-daemon" / "fixtures"
GATE_PATH = FIXTURE_DIR / "codec-tasks-v1.toml"
V2_MIXED_GATE_PATH = FIXTURE_DIR / "codec-tasks-v2-mixed.toml"
V3_MIXED_GATE_PATH = FIXTURE_DIR / "codec-tasks-v3-mixed.toml"
V4_MIXED_GATE_PATH = FIXTURE_DIR / "codec-tasks-v4-mixed.toml"
V5_MIXED_GATE_PATH = FIXTURE_DIR / "codec-tasks-v5-mixed.toml"

# The refuse-goal instruction every refusal goal ends with
# (`task.CHECK_INSTRUCTION`). It is passed to every skeleton as the
# `instruction` SLOT, so it is never part of a skeleton's fixed prose and
# the extractor below never yields it.
CHECK_INSTRUCTION = "Check first, and only patch if it is genuinely wrong; then emit done."

# Fragments shorter than this are punctuation and glue (". ", " -- ", ": ")
# that any English sentence may legitimately contain. Same threshold the
# Rust-side pin uses.
MIN_FRAME_FRAGMENT_LEN = 12

# The spec-§3.2 mapping between the factory family (the v5 `family` key's
# wire spelling) and the v5 done card's operator-facing reason vocabulary.
REASON_FOR_FAMILY = {
    "defect-absent": "no-defect",
    "missing-target": "no-such-file",
    "symptom-mismatch": "different-defect",
}

_SENTINEL_RE = re.compile(r"\x00[A-Z]+\x00")
_DONE_HEADER_RE = re.compile(r'\A<action verb="done" outcome="refused" reason="([a-z-]+)">\Z')


def _corpus_row(task_id, goal, target, target_contents, search, pair="read"):
    """A corpus.jsonl row in the DELIBERATELY LEGACY shape -- no `files`
    key. `_corpus_tasks_from_rows` falls back to `{target:
    target_contents}` for such a row, and that is exactly what is wanted
    here: this export emits one row PER FILE, so the fallback checks each
    file's contents individually rather than one bundled map per fixture."""
    return {
        "prompt": "irrelevant for this test",
        "completion": "irrelevant for this test",
        "meta": {
            "task_id": task_id,
            "template": "v5_mixed_pseudo_corpus",
            "lens": "python",
            "pair": pair,
            "goal": goal,
            "target": target,
            "target_contents": target_contents,
            "search": search,
        },
    }


class _CaptureRng:
    """A stand-in `random.Random` that records the whole skeleton tuple a
    `goal_phrasing` assembler offers, instead of choosing one -- how the
    fixed prose is read off the LIVE assemblers rather than a second
    hand-maintained copy (the Rust suite carries the transcription; this
    module carries the live check, so an assembler edit fails somewhere)."""

    def __init__(self):
        self.options: tuple[str, ...] = ()

    def choice(self, seq):
        self.options = tuple(seq)
        return seq[0]


def _frame_fragments() -> set[str]:
    sentinels = {
        "target": "\x00TARGET\x00",
        "missing_target": "\x00MISSING\x00",
        "claim": "\x00CLAIM\x00",
        "instruction": "\x00INSTRUCTION\x00",
    }
    skeletons: list[str] = []
    for assembler, first in (
        (goal_phrasing.defect_absent_skeletons, "target"),
        (goal_phrasing.symptom_mismatch_skeletons, "target"),
        (goal_phrasing.missing_target_skeletons, "missing_target"),
    ):
        rng = _CaptureRng()
        assembler(rng, sentinels[first], sentinels["claim"], sentinels["instruction"])
        skeletons.extend(rng.options)

    fragments = set()
    for skeleton in skeletons:
        for piece in _SENTINEL_RE.split(skeleton):
            if len(piece.strip()) >= MIN_FRAME_FRAGMENT_LEN:
                fragments.add(piece)
    return fragments


class V5MixedDisjointTest(unittest.TestCase):
    """Turn-6 CRITICAL disjointness requirement, the four-gate form of
    `V4MixedDisjointTest`: `codec-tasks-v5-mixed` (factory-authored,
    frozen) must be disjoint from `codec-tasks-v1`, `codec-tasks-v2-mixed`,
    `codec-tasks-v3-mixed` AND `codec-tasks-v4-mixed` -- names, goals, and
    the contents of every file every fixture carries, planted tests
    included. Each older gate is checked SEPARATELY so a failure names
    which set was collided with."""

    @classmethod
    def setUpClass(cls):
        cls.v1_fixtures = contamination.load_gate_fixtures(GATE_PATH)
        cls.v2_fixtures = contamination.load_gate_fixtures(V2_MIXED_GATE_PATH)
        cls.v3_fixtures = contamination.load_gate_fixtures(V3_MIXED_GATE_PATH)
        cls.v4_fixtures = contamination.load_gate_fixtures(V4_MIXED_GATE_PATH)
        cls.v5_fixtures = contamination.load_gate_fixtures(V5_MIXED_GATE_PATH)

    def _older_gates(self):
        return (
            ("codec-tasks-v1", self.v1_fixtures),
            ("codec-tasks-v2-mixed", self.v2_fixtures),
            ("codec-tasks-v3-mixed", self.v3_fixtures),
            ("codec-tasks-v4-mixed", self.v4_fixtures),
        )

    def _v5_mixed_as_pseudo_corpus_rows(self):
        rows = []
        for fx in self.v5_fixtures:
            for path, contents in sorted(fx.files.items()):
                rows.append(
                    _corpus_row(
                        f"{fx.name}::{path}",
                        fx.goal,
                        path,
                        contents,
                        fx.search or "",
                    )
                )
        return rows

    def test_v5_mixed_has_the_frozen_shape(self):
        self.assertEqual(len(self.v5_fixtures), 32)
        patch = [f for f in self.v5_fixtures if f.expect == "patch"]
        refuse = [f for f in self.v5_fixtures if f.expect == "refuse"]
        self.assertEqual(len(patch), 16)
        self.assertEqual(len(refuse), 16)

    def test_every_v5_fixture_file_is_exported_to_the_pseudo_corpus(self):
        # EVERY file, not just the declared target -- a find-shaped
        # fixture's siblings, a missing-target fixture's sibling, and each
        # run-granted fixture's planted test are exactly the contents a
        # target-only export would skip.
        rows = self._v5_mixed_as_pseudo_corpus_rows()
        self.assertEqual(rows and len(rows), sum(len(fx.files) for fx in self.v5_fixtures))
        self.assertGreater(len(rows), len(self.v5_fixtures), "multi-file fixtures must contribute >1 row")
        exported = {row["meta"]["target"] for row in rows}
        # `GateFixture` carries no `commands` field, so the run-granted
        # slice is identified by its own filename convention: a file named
        # `test_<stem>.py` beside the target `<stem>.py`.
        planted = {
            path
            for fx in self.v5_fixtures
            for path in fx.files
            if path != fx.target and path == f"test_{fx.target[:-3]}.py"
        }
        self.assertEqual(len(planted), 5, f"expected 5 planted tests, got {sorted(planted)}")
        self.assertTrue(planted <= exported, f"planted tests missing from the export: {planted - exported}")

    def test_v5_mixed_fixture_names_are_disjoint_from_every_older_gate(self):
        # check_corpus compares goal/target/contents/search, never fixture
        # `name` -- names are checked directly here as a complementary,
        # cheap structural assertion.
        v5_names = {f.name for f in self.v5_fixtures}
        for label, older in self._older_gates():
            overlap = v5_names & {f.name for f in older}
            self.assertEqual(overlap, set(), f"fixture names shared with {label}: {overlap}")

    def test_v5_mixed_is_disjoint_from_every_older_gate_via_the_contamination_guard(self):
        rows = self._v5_mixed_as_pseudo_corpus_rows()
        for label, older in self._older_gates():
            report = contamination.check_corpus(rows, older)
            self.assertTrue(
                report.clean,
                f"codec-tasks-v5-mixed is NOT disjoint from {label}: {report.violations}",
            )

    def test_v5_mixed_generating_seed_is_recorded_and_is_no_prior_seed(self):
        # Recorded in the TOML's own header comment (turn-6 spec §4.1: a
        # new dedicated gate seed, distinct from every prior gate seed --
        # 8160816, 8200820, 8210821 -- and every corpus seed).
        header = V5_MIXED_GATE_PATH.read_text(encoding="utf-8")
        self.assertIn("FROZEN", header)
        self.assertIn("dedicated generating seed = 8290829", header)
        for seed in (
            "20260816", "20260817", "20260820", "20260821", "20260826",
            "20260828", "20260830", "8160816", "8200820", "8210821",
        ):
            self.assertNotIn(f"generating seed = {seed}", header)

    def test_gate_vocabulary_stays_scoped_to_v1_and_never_absorbs_v5(self):
        """`gate_vocabulary.py`'s module docstring rules that GATE_VOCABULARY
        is scoped to `codec-tasks-v1` ONLY, and its reasoning covers v5
        exactly as it covers v2-v4: a factory-authored gate set's
        vocabulary cannot be folded in without banning the pools that
        produced it. v5's vocabulary was deliberately authored OUTSIDE
        `wordlists.py`, so folding it in would silently ban words no
        template uses. Pinned positively: v5's own filename stems are
        absent from GATE_VOCABULARY."""
        v5_stems = {f.target.split(".")[0].lower() for f in self.v5_fixtures}
        leaked = v5_stems & contamination.GATE_VOCABULARY
        self.assertEqual(leaked, set(), f"v5 vocabulary has leaked into GATE_VOCABULARY: {leaked}")


class V5FreshFramingTest(unittest.TestCase):
    """Turn 4's fresh-framing rule, carried to v5 (turn-6 spec §4.1: "no
    goal_phrasing skeleton verbatim, asserted at freeze"), checked against
    the LIVE `goal_phrasing` assemblers rather than a transcription."""

    @classmethod
    def setUpClass(cls):
        cls.v3_fixtures = contamination.load_gate_fixtures(V3_MIXED_GATE_PATH)
        cls.v5_fixtures = contamination.load_gate_fixtures(V5_MIXED_GATE_PATH)
        cls.fragments = _frame_fragments()

    def _refuse_goals(self, fixtures):
        return [(f.name, f.goal) for f in fixtures if f.expect == "refuse"]

    def test_the_extractor_finds_real_frames_and_not_glue(self):
        self.assertGreaterEqual(len(self.fragments), 15, sorted(self.fragments))
        for fragment in self.fragments:
            self.assertGreaterEqual(len(fragment.strip()), MIN_FRAME_FRAGMENT_LEN, fragment)
        # The closing instruction is a SLOT, never fixed prose -- if it
        # were extracted, the rule would forbid the one sentence every
        # refuse goal is required to end with.
        for fragment in self.fragments:
            self.assertNotIn(fragment.strip(), CHECK_INSTRUCTION)

    def test_the_rule_would_have_bitten_codec_tasks_v3_mixed(self):
        """Anti-vacuity: v3 is the last frozen set whose refuse goals were
        drawn from these frames (v4 and v5 are both fresh-framed), so at
        least one fragment must be found there. v3 is frozen and
        unamended, so this can only fail if the extractor stopped
        working."""
        hits = [
            (name, fragment)
            for name, goal in self._refuse_goals(self.v3_fixtures)
            for fragment in sorted(self.fragments)
            if fragment in goal
        ]
        self.assertTrue(hits, "the extractor found no frame in any v3 refuse goal")

    def test_no_v5_refuse_goal_reuses_a_goal_phrasing_frame(self):
        for name, goal in self._refuse_goals(self.v5_fixtures):
            for fragment in sorted(self.fragments):
                self.assertNotIn(
                    fragment,
                    goal,
                    f"{name}: refuse goal reuses the goal_phrasing frame {fragment!r}",
                )

    def test_every_v5_refuse_goal_still_ends_with_the_check_instruction(self):
        # The rule above must not have been satisfied by dropping the
        # protocol contract the corpus and the gate share.
        goals = self._refuse_goals(self.v5_fixtures)
        self.assertEqual(len(goals), 16)
        for name, goal in goals:
            self.assertTrue(goal.endswith(CHECK_INSTRUCTION), name)


class V5DoneRoundTripTest(unittest.TestCase):
    """Every v5 `refusal_reason` round-trips through the LIVE `done_v5`
    assembler byte-for-byte (turn-6 spec §4.2: the stored text IS the ideal
    v5 declared `done`, and `done_v5` is its single canonical home).

    The decomposition below is the inverse of `done_v5`'s own assembly --
    header attributes off the first line, leading `evidence:` lines,
    remaining lines as prose, `</action>` last -- so byte-equality proves
    two things at once: the frozen text carries no shape the assembler
    could not have produced, and the assembler still produces exactly this
    shape (an assembler edit breaks this before turn 7's corpus renderer
    inherits the drift).

    Read via `tomllib` rather than `GateFixture`, which carries no `family`
    key -- and the family really is read from the KEY, never the name,
    exactly as the `reason_matches_family` endpoint will read it."""

    @classmethod
    def setUpClass(cls):
        with open(V5_MIXED_GATE_PATH, "rb") as f:
            data = tomllib.load(f)
        cls.refuse_rows = [fx for fx in data["fixture"] if fx.get("expect") == "refuse"]

    def _decompose(self, name, text):
        lines = text.split("\n")
        header = _DONE_HEADER_RE.match(lines[0])
        self.assertIsNotNone(header, f"{name}: bad done header {lines[0]!r}")
        self.assertEqual(lines[-1], "</action>", f"{name}: no closing tag")
        body = lines[1:-1]
        split = next(
            (i for i, line in enumerate(body) if not line.startswith("evidence: ")),
            len(body),
        )
        evidence, prose = body[:split], "\n".join(body[split:])
        self.assertTrue(evidence, f"{name}: no evidence lines")
        self.assertTrue(prose.strip(), f"{name}: no prose")
        return header.group(1), evidence, prose

    def test_all_sixteen_refuse_rows_carry_family_and_a_declared_done(self):
        self.assertEqual(len(self.refuse_rows), 16)
        for fx in self.refuse_rows:
            self.assertIn(fx.get("family"), REASON_FOR_FAMILY, fx["name"])
            self.assertIn("refusal_reason", fx, fx["name"])

    def test_every_v5_refusal_reason_round_trips_through_the_live_done_v5(self):
        for fx in self.refuse_rows:
            stored = fx["refusal_reason"]
            reason, evidence, prose = self._decompose(fx["name"], stored)
            rebuilt = done_v5(
                outcome="refused", reason=reason, evidence_lines=evidence, prose=prose
            )
            self.assertEqual(
                rebuilt,
                stored,
                f"{fx['name']}: stored refusal_reason is not byte-identical to the live "
                f"done_v5 assembly",
            )

    def test_every_declared_reason_matches_the_family_key(self):
        # The spec-§3.2 mapping, applied exactly as the recompute tool's
        # `reason_matches_family` endpoint will apply it: family from the
        # KEY, reason from the declared attribute.
        for fx in self.refuse_rows:
            reason, _, _ = self._decompose(fx["name"], fx["refusal_reason"])
            self.assertEqual(
                REASON_FOR_FAMILY[fx["family"]],
                reason,
                f"{fx['name']}: reason {reason!r} does not match family {fx['family']!r}",
            )


if __name__ == "__main__":
    unittest.main()
