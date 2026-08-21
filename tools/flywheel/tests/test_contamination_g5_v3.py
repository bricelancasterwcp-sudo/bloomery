"""`codec-tasks-v3-mixed`'s disjointness from BOTH older gate sets (turn-3
task-8 brief), split out of `test_contamination_g5.py` for the same 400-line
house cap that split THAT file out of `test_contamination.py`.

The mechanism is `V2MixedDisjointFromV1GuardTest`'s, applied a third time and
against two gates instead of one: v3-mixed is factory-authored, not a training
corpus, so the guard's normal direction ("corpus vs gate") inverts -- export
every v3-mixed FILE as a pseudo-corpus row and run the SAME `check_corpus`
comparator, once with `codec-tasks-v1` as the gate and once with
`codec-tasks-v2-mixed`.

Turn 4's `codec-tasks-v4-mixed` gets the same treatment against all three
older sets, one file further along the same chain:
`test_contamination_g5_v4.py`.
"""

import unittest
from pathlib import Path

from tools.flywheel.factory import contamination

REPO_ROOT = Path(__file__).resolve().parents[3]
GATE_PATH = REPO_ROOT / "crates" / "bloomery-daemon" / "fixtures" / "codec-tasks-v1.toml"
V2_MIXED_GATE_PATH = REPO_ROOT / "crates" / "bloomery-daemon" / "fixtures" / "codec-tasks-v2-mixed.toml"
V3_MIXED_GATE_PATH = REPO_ROOT / "crates" / "bloomery-daemon" / "fixtures" / "codec-tasks-v3-mixed.toml"


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
            "template": "v3_mixed_pseudo_corpus",
            "lens": "python",
            "pair": pair,
            "goal": goal,
            "target": target,
            "target_contents": target_contents,
            "search": search,
        },
    }


class V3MixedDisjointTest(unittest.TestCase):
    """Turn-3 task-8 CRITICAL disjointness requirement, the two-gate form of
    `V2MixedDisjointFromV1GuardTest` above: `codec-tasks-v3-mixed`
    (factory-authored, frozen) must be disjoint from `codec-tasks-v1` AND
    from `codec-tasks-v2-mixed` — names, goals, and the contents of every
    file every fixture carries.

    Same inversion and same mechanism as the v2 test: v3-mixed is
    FACTORY-MADE, not a training corpus, so the guard's normal direction
    ("corpus vs gate") flips — export every v3-mixed FILE as a pseudo-corpus
    row and run the SAME `check_corpus` comparator with the older gate as
    the gate. Each older gate is checked SEPARATELY rather than as one
    union, so a failure names which set was collided with instead of
    leaving that to be read out of the violation list.
    """

    def setUp(self):
        self.v1_fixtures = contamination.load_gate_fixtures(GATE_PATH)
        self.v2_fixtures = contamination.load_gate_fixtures(V2_MIXED_GATE_PATH)
        self.v3_fixtures = contamination.load_gate_fixtures(V3_MIXED_GATE_PATH)

    def _v3_mixed_as_pseudo_corpus_rows(self):
        rows = []
        for fx in self.v3_fixtures:
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

    def test_v3_mixed_has_the_frozen_shape(self):
        self.assertEqual(len(self.v3_fixtures), 32)
        patch = [f for f in self.v3_fixtures if f.expect == "patch"]
        refuse = [f for f in self.v3_fixtures if f.expect == "refuse"]
        self.assertEqual(len(patch), 16)
        self.assertEqual(len(refuse), 16)

    def test_every_v3_fixture_file_is_exported_to_the_pseudo_corpus(self):
        # The rule the v2 test's docstring states but never asserts: EVERY
        # file, not just the declared target -- a find-shaped fixture's
        # siblings and a missing-target fixture's sibling are exactly the
        # contents a target-only export would skip.
        rows = self._v3_mixed_as_pseudo_corpus_rows()
        self.assertEqual(rows and len(rows), sum(len(fx.files) for fx in self.v3_fixtures))
        self.assertGreater(len(rows), len(self.v3_fixtures), "multi-file fixtures must contribute >1 row")

    def test_v3_mixed_fixture_names_are_disjoint_from_both_older_gates(self):
        # check_corpus compares goal/target/contents/search, never fixture
        # `name` -- names are checked directly here as a complementary,
        # cheap structural assertion (the brief's "names, goals, contents").
        v3_names = {f.name for f in self.v3_fixtures}
        for label, older in (("codec-tasks-v1", self.v1_fixtures), ("codec-tasks-v2-mixed", self.v2_fixtures)):
            overlap = v3_names & {f.name for f in older}
            self.assertEqual(overlap, set(), f"fixture names shared with {label}: {overlap}")

    def test_v3_mixed_is_disjoint_from_v1_via_the_contamination_guard(self):
        report = contamination.check_corpus(self._v3_mixed_as_pseudo_corpus_rows(), self.v1_fixtures)
        self.assertTrue(
            report.clean,
            f"codec-tasks-v3-mixed is NOT disjoint from codec-tasks-v1: {report.violations}",
        )

    def test_v3_mixed_is_disjoint_from_v2_mixed_via_the_contamination_guard(self):
        report = contamination.check_corpus(self._v3_mixed_as_pseudo_corpus_rows(), self.v2_fixtures)
        self.assertTrue(
            report.clean,
            f"codec-tasks-v3-mixed is NOT disjoint from codec-tasks-v2-mixed: {report.violations}",
        )

    def test_v3_mixed_generating_seed_is_recorded_and_is_no_corpus_seed(self):
        # Recorded in the TOML's own header comment (task-8 brief: the gate
        # seed 8200820 is held out and must differ from every corpus seed --
        # turn 1's 20260816, turn 2's 20260817, turn 3's 20260820 -- and from
        # turn 2's own gate seed 8160816).
        header = V3_MIXED_GATE_PATH.read_text(encoding="utf-8")
        self.assertIn("FROZEN", header)
        self.assertIn("dedicated generating seed = 8200820", header)
        for corpus_seed in ("20260816", "20260817", "20260820"):
            self.assertNotIn(f"generating seed = {corpus_seed}", header)

    def test_gate_vocabulary_stays_scoped_to_v1_and_never_absorbs_v3(self):
        """`gate_vocabulary.py`'s module docstring rules that GATE_VOCABULARY
        is scoped to `codec-tasks-v1` ONLY, and its reasoning covers v3
        exactly as it covers v2: a factory-authored gate set's vocabulary
        cannot be folded in without banning the pools that produced it.

        v3 is the harder case and the reason this pin exists: its vocabulary
        was deliberately authored OUTSIDE `wordlists.py`, so folding it in
        would NOT trip `templates.py`'s import-time assert -- it would
        silently succeed and quietly ban a set of words no template uses,
        making the ban look free while the rule it encodes ("never union a
        factory-authored gate") had been broken. Pinned positively here: v3's
        own filename stems are absent from GATE_VOCABULARY.
        """
        v3_stems = {f.target.split(".")[0].lower() for f in self.v3_fixtures}
        leaked = v3_stems & contamination.GATE_VOCABULARY
        self.assertEqual(leaked, set(), f"v3 vocabulary has leaked into GATE_VOCABULARY: {leaked}")


if __name__ == "__main__":
    unittest.main()
