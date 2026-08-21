"""`codec-tasks-v4-mixed`'s disjointness from ALL THREE older gate sets
(turn-4 task-5 brief), split out of `test_contamination_g5.py` for the same
400-line house cap that split THAT file out of `test_contamination.py` and
`test_contamination_g5_v3.py` out of it in turn.

The mechanism is `V3MixedDisjointTest`'s, applied a fourth time and against
three gates instead of two: v4-mixed is factory-authored, not a training
corpus, so the guard's normal direction ("corpus vs gate") inverts -- export
every v4-mixed FILE as a pseudo-corpus row and run the SAME `check_corpus`
comparator, once per older gate set.

Turn 4 adds two things this module is the right home for:

- the run slice plants a `test_<stem>.py` beside each run-granted target,
  so the export carries files the v3 export never had, and the widened
  filename rule (`contamination._violations_for_task`'s `names_norm`)
  screens them against every gate target;
- the FRESH-FRAMING rule is re-checked here against the REAL
  `goal_phrasing` module. `codec_fixtures_v4_rules_test.rs` pins the same
  rule against the frozen bytes with the skeletons TRANSCRIBED (the Rust
  crate has no dependency on this factory); this module extracts them from
  the live assemblers instead, so a factory-side edit that the Rust
  transcription missed still fails somewhere.
"""

import re
import unittest
from pathlib import Path

from tools.flywheel.factory import contamination, goal_phrasing

REPO_ROOT = Path(__file__).resolve().parents[3]
FIXTURE_DIR = REPO_ROOT / "crates" / "bloomery-daemon" / "fixtures"
GATE_PATH = FIXTURE_DIR / "codec-tasks-v1.toml"
V2_MIXED_GATE_PATH = FIXTURE_DIR / "codec-tasks-v2-mixed.toml"
V3_MIXED_GATE_PATH = FIXTURE_DIR / "codec-tasks-v3-mixed.toml"
V4_MIXED_GATE_PATH = FIXTURE_DIR / "codec-tasks-v4-mixed.toml"

# The refuse-goal instruction every refusal goal ends with
# (`task.CHECK_INSTRUCTION`). It is passed to every skeleton as the
# `instruction` SLOT, so it is never part of a skeleton's fixed prose and
# the extractor below never yields it -- which is what makes the rule
# compatible with the protocol contract that every refuse goal must end
# with it.
CHECK_INSTRUCTION = "Check first, and only patch if it is genuinely wrong; then emit done."

# Fragments shorter than this are punctuation and glue (". ", " -- ", ": ")
# that any English sentence may legitimately contain. Same threshold the
# Rust-side pin uses.
MIN_FRAME_FRAGMENT_LEN = 12

_SENTINEL_RE = re.compile(r"\x00[A-Z]+\x00")


def _corpus_row(task_id, goal, target, target_contents, search, pair="read"):
    """A corpus.jsonl row in the DELIBERATELY LEGACY shape -- no `files` key.
    `_corpus_tasks_from_rows` falls back to `{target: target_contents}` for
    such a row, and that is exactly what is wanted here: this export emits one
    row PER FILE, so the fallback checks each file's contents individually
    rather than re-checking one bundled map per fixture."""
    return {
        "prompt": "irrelevant for this test",
        "completion": "irrelevant for this test",
        "meta": {
            "task_id": task_id,
            "template": "v4_mixed_pseudo_corpus",
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
    `goal_phrasing` assembler offers, instead of choosing one of them.

    This is how the fixed prose is read off the LIVE assemblers rather than
    a second hand-maintained copy: call each assembler with sentinel
    arguments, capture every skeleton it built, and split each on the
    sentinels. Anything left is fixed prose the skeleton contributes."""

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


class V4MixedDisjointTest(unittest.TestCase):
    """Turn-4 task-5 CRITICAL disjointness requirement, the three-gate form
    of `V3MixedDisjointTest`: `codec-tasks-v4-mixed` (factory-authored,
    frozen) must be disjoint from `codec-tasks-v1`, `codec-tasks-v2-mixed`
    AND `codec-tasks-v3-mixed` — names, goals, and the contents of every
    file every fixture carries, planted tests included.

    Each older gate is checked SEPARATELY rather than as one union, so a
    failure names which set was collided with instead of leaving that to be
    read out of the violation list.
    """

    @classmethod
    def setUpClass(cls):
        cls.v1_fixtures = contamination.load_gate_fixtures(GATE_PATH)
        cls.v2_fixtures = contamination.load_gate_fixtures(V2_MIXED_GATE_PATH)
        cls.v3_fixtures = contamination.load_gate_fixtures(V3_MIXED_GATE_PATH)
        cls.v4_fixtures = contamination.load_gate_fixtures(V4_MIXED_GATE_PATH)

    def _older_gates(self):
        return (
            ("codec-tasks-v1", self.v1_fixtures),
            ("codec-tasks-v2-mixed", self.v2_fixtures),
            ("codec-tasks-v3-mixed", self.v3_fixtures),
        )

    def _v4_mixed_as_pseudo_corpus_rows(self):
        rows = []
        for fx in self.v4_fixtures:
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

    def test_v4_mixed_has_the_frozen_shape(self):
        self.assertEqual(len(self.v4_fixtures), 32)
        patch = [f for f in self.v4_fixtures if f.expect == "patch"]
        refuse = [f for f in self.v4_fixtures if f.expect == "refuse"]
        self.assertEqual(len(patch), 16)
        self.assertEqual(len(refuse), 16)

    def test_every_v4_fixture_file_is_exported_to_the_pseudo_corpus(self):
        # EVERY file, not just the declared target -- a find-shaped
        # fixture's siblings, a missing-target fixture's sibling, and (new
        # in turn 4) each run-granted fixture's planted test are exactly the
        # contents a target-only export would skip.
        rows = self._v4_mixed_as_pseudo_corpus_rows()
        self.assertEqual(rows and len(rows), sum(len(fx.files) for fx in self.v4_fixtures))
        self.assertGreater(len(rows), len(self.v4_fixtures), "multi-file fixtures must contribute >1 row")
        exported = {row["meta"]["target"] for row in rows}
        # `GateFixture` carries no `commands` field, so the run-granted
        # slice is identified the way its own filename convention states it:
        # a file named `test_<stem>.py` beside the target `<stem>.py`.
        planted = {
            path
            for fx in self.v4_fixtures
            for path in fx.files
            if path != fx.target and path == f"test_{fx.target[:-3]}.py"
        }
        self.assertEqual(len(planted), 5, f"expected 5 planted tests, got {sorted(planted)}")
        self.assertTrue(planted <= exported, f"planted tests missing from the export: {planted - exported}")

    def test_v4_mixed_fixture_names_are_disjoint_from_every_older_gate(self):
        # check_corpus compares goal/target/contents/search, never fixture
        # `name` -- names are checked directly here as a complementary,
        # cheap structural assertion (the brief's "names, goals, contents").
        v4_names = {f.name for f in self.v4_fixtures}
        for label, older in self._older_gates():
            overlap = v4_names & {f.name for f in older}
            self.assertEqual(overlap, set(), f"fixture names shared with {label}: {overlap}")

    def test_v4_mixed_is_disjoint_from_every_older_gate_via_the_contamination_guard(self):
        rows = self._v4_mixed_as_pseudo_corpus_rows()
        for label, older in self._older_gates():
            report = contamination.check_corpus(rows, older)
            self.assertTrue(
                report.clean,
                f"codec-tasks-v4-mixed is NOT disjoint from {label}: {report.violations}",
            )

    def test_v4_mixed_generating_seed_is_recorded_and_is_no_corpus_seed(self):
        # Recorded in the TOML's own header comment (task-5 brief: the gate
        # seed 8210821 is held out and must differ from every corpus seed --
        # turn 1's 20260816, turn 2's 20260817, turn 3's 20260820, turn 4's
        # 20260821 -- and from turns 2 and 3's own gate seeds).
        header = V4_MIXED_GATE_PATH.read_text(encoding="utf-8")
        self.assertIn("FROZEN", header)
        self.assertIn("dedicated generating seed = 8210821", header)
        for seed in ("20260816", "20260817", "20260820", "20260821", "8160816", "8200820"):
            self.assertNotIn(f"generating seed = {seed}", header)

    def test_gate_vocabulary_stays_scoped_to_v1_and_never_absorbs_v4(self):
        """`gate_vocabulary.py`'s module docstring rules that GATE_VOCABULARY
        is scoped to `codec-tasks-v1` ONLY, and its reasoning covers v4
        exactly as it covers v2 and v3: a factory-authored gate set's
        vocabulary cannot be folded in without banning the pools that
        produced it. v4's vocabulary was deliberately authored OUTSIDE
        `wordlists.py`, so folding it in would NOT trip `templates.py`'s
        import-time assert -- it would silently succeed and quietly ban a set
        of words no template uses. Pinned positively: v4's own filename stems
        are absent from GATE_VOCABULARY."""
        v4_stems = {f.target.split(".")[0].lower() for f in self.v4_fixtures}
        leaked = v4_stems & contamination.GATE_VOCABULARY
        self.assertEqual(leaked, set(), f"v4 vocabulary has leaked into GATE_VOCABULARY: {leaked}")


class V4FreshFramingTest(unittest.TestCase):
    """Turn 4's fresh-framing rule (design spec §4), checked against the
    LIVE `goal_phrasing` assemblers rather than a transcription of them."""

    @classmethod
    def setUpClass(cls):
        cls.v3_fixtures = contamination.load_gate_fixtures(V3_MIXED_GATE_PATH)
        cls.v4_fixtures = contamination.load_gate_fixtures(V4_MIXED_GATE_PATH)
        cls.fragments = _frame_fragments()

    def _refuse_goals(self, fixtures):
        return [(f.name, f.goal) for f in fixtures if f.expect == "refuse"]

    def test_the_extractor_finds_real_frames_and_not_glue(self):
        self.assertGreaterEqual(len(self.fragments), 15, sorted(self.fragments))
        for fragment in self.fragments:
            self.assertGreaterEqual(len(fragment.strip()), MIN_FRAME_FRAGMENT_LEN, fragment)
        # The closing instruction is a SLOT, never fixed prose -- if it were
        # extracted, the rule would forbid the one sentence every refuse
        # goal is required to end with.
        for fragment in self.fragments:
            self.assertNotIn(fragment.strip(), CHECK_INSTRUCTION)

    def test_the_rule_would_have_bitten_codec_tasks_v3_mixed(self):
        """Anti-vacuity, and the reason the rule exists: v3's refuse goals
        were drawn from these frames (the v3 evidence review named exactly
        this), so at least one fragment must be found there. v3 is frozen
        and unamended, so this can only fail if the extractor stopped
        working."""
        hits = [
            (name, fragment)
            for name, goal in self._refuse_goals(self.v3_fixtures)
            for fragment in sorted(self.fragments)
            if fragment in goal
        ]
        self.assertTrue(hits, "the extractor found no frame in any v3 refuse goal")

    def test_no_v4_refuse_goal_reuses_a_goal_phrasing_frame(self):
        for name, goal in self._refuse_goals(self.v4_fixtures):
            for fragment in sorted(self.fragments):
                self.assertNotIn(
                    fragment,
                    goal,
                    f"{name}: refuse goal reuses the goal_phrasing frame {fragment!r}",
                )

    def test_every_v4_refuse_goal_still_ends_with_the_check_instruction(self):
        # The rule above must not have been satisfied by dropping the
        # protocol contract the corpus and the gate share.
        goals = self._refuse_goals(self.v4_fixtures)
        self.assertEqual(len(goals), 16)
        for name, goal in goals:
            self.assertTrue(goal.endswith(CHECK_INSTRUCTION), name)


if __name__ == "__main__":
    unittest.main()
