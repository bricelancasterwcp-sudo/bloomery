"""Tests for turn 3's repair-trajectory slice cycle in generate.py
(task-7 brief; turn-3 design doc §2).

The patch slice is no longer one shape: it cycles plain -> find-shaped ->
run-verified by SLOT POSITION (never by rng draw, exactly like the
python:plaintext `_FAMILY_PATTERN` it sits beside), so a run of `--count
999` is 333 of each and determinism (rule 3) never depends on draw order.
Each shape renders a different number of pairs -- 3 plain, 4 find, 4 run
-- and the corpus/fingerprint must say which shape produced which row.

Split out of `test_generate.py` (already at the 400-line house cap) the
same way `test_generate_gates.py` and `test_generate_refusal.py` were; it
imports the shared `STUB_TOOL`/`REAL_TOOL`/`run_generate` helpers from it.
"""

import json
import re
import subprocess
import sys
import tempfile
import unittest
from collections import Counter
from pathlib import Path

from tools.flywheel.factory import generate, generate_request, generate_slices
from tools.flywheel.factory.task import Task
from tools.flywheel.tests.test_generate import REAL_TOOL, STUB_TOOL, run_generate

PAIRS_BY_SHAPE = {
    "plain": ["read", "patch", "done"],
    "find": ["find", "read", "patch", "done"],
    "run": ["read", "patch", "run", "done"],
}


def _generate(tmp, args):
    out, report = tmp / "out.jsonl", tmp / "report.json"
    result = run_generate([*args, "--out", str(out), "--report", str(report)])
    return result, out, report


def _rows_by_task(out):
    by_task = {}
    for line in out.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        by_task.setdefault(row["meta"]["task_id"], []).append(row)
    return by_task


class SliceCycleIsPositionDerivedTest(unittest.TestCase):
    def test_the_cycle_is_an_even_three_way_split_at_a_multiple_of_three(self):
        shapes = Counter(generate_slices.trajectory_for_slot(i) for i in range(999))
        self.assertEqual(shapes, Counter({"plain": 333, "find": 333, "run": 333}))

    def test_slot_zero_is_still_the_plain_python_family(self):
        # `test_generate_gates.py` replays `templates.PYTHON_TEMPLATES[0]`
        # against slot 0 to predict what a `--count 1` run produces; the
        # cycle must keep that slot plain or that prediction silently
        # stops describing the CLI.
        self.assertEqual(generate_slices.trajectory_for_slot(0), "plain")

    def test_every_slot_gets_a_family_from_its_own_shape_registry(self):
        for count in (1, 5, 30):
            fns = generate_slices.family_functions(count)
            self.assertEqual(len(fns), count)


class ShapeMixEndToEndTest(unittest.TestCase):
    def test_a_run_spanning_the_cycle_produces_all_three_shapes(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            result, out, report = _generate(
                tmp, ["--seed", "17", "--count", "30", "--tool", str(STUB_TOOL)]
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            fp = json.loads(report.read_text(encoding="utf-8"))
            self.assertEqual(fp["tasks_by_trajectory"], {"find": 10, "plain": 10, "run": 10})

            shapes = {row["meta"]["trajectory"] for rows in _rows_by_task(out).values() for row in rows}
            self.assertEqual(shapes, {"plain", "find", "run"})

    def test_each_task_yields_exactly_the_pair_sequence_for_its_shape(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            result, out, report = _generate(
                tmp, ["--seed", "17", "--count", "30", "--tool", str(STUB_TOOL)]
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            fp = json.loads(report.read_text(encoding="utf-8"))
            by_task = _rows_by_task(out)
            self.assertEqual(sum(len(rows) for rows in by_task.values()), fp["pairs"])
            for task_id, rows in by_task.items():
                shape = rows[0]["meta"]["trajectory"]
                self.assertEqual(
                    [row["meta"]["pair"] for row in rows], PAIRS_BY_SHAPE[shape], task_id
                )

    def test_find_rows_carry_every_sibling_file_in_their_meta(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            result, out, _report = _generate(
                tmp, ["--seed", "17", "--count", "30", "--tool", str(STUB_TOOL)]
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            find_tasks = [
                rows for rows in _rows_by_task(out).values() if rows[0]["meta"]["trajectory"] == "find"
            ]
            self.assertTrue(find_tasks)
            for rows in find_tasks:
                meta = rows[0]["meta"]
                self.assertGreaterEqual(len(meta["files"]), 3, meta["task_id"])
                self.assertIn(meta["target"], meta["files"])

    def test_same_seed_still_produces_a_byte_identical_corpus_across_the_new_shapes(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            args = ["--seed", "23", "--count", "12", "--tool", str(STUB_TOOL)]
            out_a, report_a = tmp / "a.jsonl", tmp / "a.json"
            out_b, report_b = tmp / "b.jsonl", tmp / "b.json"
            a = run_generate([*args, "--out", str(out_a), "--report", str(report_a)])
            b = run_generate([*args, "--out", str(out_b), "--report", str(report_b)])
            self.assertEqual(a.returncode, 0, a.stderr)
            self.assertEqual(b.returncode, 0, b.stderr)
            self.assertEqual(out_a.read_bytes(), out_b.read_bytes())
            self.assertEqual(report_a.read_bytes(), report_b.read_bytes())


class LandingFailureAbortsEveryShapeTest(unittest.TestCase):
    """The turn-3 wire contract's partial response: a reference patch that
    does not land comes back as `landed: false` + `landing_detail` + the
    pairs built SO FAR (3 for the find shape, 2 for the run shape), not as
    an error. The generation loop must treat that exactly as it treats the
    plain shape's — a hard abort, never a short task quietly written to the
    corpus — so the pair-count check must never get there first."""

    CONTENTS = "def add(a, b):\n    return a + b\n\n\ndef sub(a, b):\n    return a - b\n"

    def _task(self, **extra):
        return Task(
            name="fam",
            lens="python",
            target="mathy.py",
            files={"mathy.py": self.CONTENTS},
            goal="TRIGGER_LANDING_FAILURE add() is broken. Patch the file, then emit done.",
            search="    return a + b",
            replace="    return a + b  # ok",
            summary="Fixed add().",
            **extra,
        )

    def _partial_pairs(self, task):
        request = generate_request.build_trajectory_request(task)
        proc = subprocess.run(
            [sys.executable, str(STUB_TOOL)],
            input=json.dumps(request) + "\n",
            capture_output=True,
            text=True,
        )
        return json.loads(proc.stdout.splitlines()[0])

    def test_find_shape_returns_three_pairs_so_far_and_aborts_the_run(self):
        task = self._task(trajectory="find", find_pattern="def add")
        response = self._partial_pairs(task)
        self.assertFalse(response["landed"])
        self.assertEqual(len(response["pairs"]), 3)
        self.assertIn("landing_detail", response)
        with self.assertRaises(SystemExit):
            generate._verify_and_build_rows([("t1", task)], STUB_TOOL)

    def test_run_shape_returns_two_pairs_so_far_and_aborts_the_run(self):
        task = self._task(
            trajectory="run",
            run_argv=("python3", "-m", "py_compile", "mathy.py"),
            commands=(("python3", "-m", "py_compile"),),
        )
        response = self._partial_pairs(task)
        self.assertFalse(response["landed"])
        self.assertEqual(len(response["pairs"]), 2)
        self.assertIn("landing_detail", response)
        with self.assertRaises(SystemExit):
            generate._verify_and_build_rows([("t1", task)], STUB_TOOL)


@unittest.skipUnless(
    REAL_TOOL is not None,
    "flywheel-tool binary not built; run cargo build --release -p bloomery-daemon --bin flywheel-tool",
)
class RealToolTrajectoryShapesTest(unittest.TestCase):
    """The brief's end-to-end row through the REAL binary, one per new
    shape. The stub can only echo canned text; only the real tool proves
    the `find` observation came from a real `exec_find` walk and the `run`
    observation from a real `exec_run` of `py_compile` against the PATCHED
    file."""

    @classmethod
    def setUpClass(cls):
        cls._tmp = tempfile.TemporaryDirectory()
        tmp = Path(cls._tmp.name)
        result, out, report = _generate(
            tmp, ["--seed", "5", "--count", "12", "--tool", str(REAL_TOOL)]
        )
        assert result.returncode == 0, result.stderr
        cls.by_task = _rows_by_task(out)
        cls.fingerprint = json.loads(report.read_text(encoding="utf-8"))

    @classmethod
    def tearDownClass(cls):
        cls._tmp.cleanup()

    def _tasks_of_shape(self, shape):
        rows = [r for r in self.by_task.values() if r[0]["meta"]["trajectory"] == shape]
        self.assertTrue(rows, f"no {shape}-shaped task in the run")
        return rows

    def test_find_shaped_rows_open_with_a_real_find_observation(self):
        for rows in self._tasks_of_shape("find"):
            find_row, read_row = rows[0], rows[1]
            pattern = find_row["meta"]["find_pattern"]
            self.assertIn('<action verb="find"', find_row["completion"])
            self.assertIn(f'pattern="{pattern}"', find_row["completion"])
            self.assertIn('path="."', find_row["completion"])
            # Pair 2's prompt carries the transcript of pair 1's real
            # `exec_find` observation: its outcome line plus at least one
            # `path:lineno: line` hit naming the target.
            self.assertIn("found 1 matches", read_row["prompt"])
            self.assertIn(find_row["meta"]["target"], read_row["prompt"])
            self.assertIn(pattern, read_row["prompt"])

    def test_run_verified_rows_verify_the_patched_file_before_done(self):
        for rows in self._tasks_of_shape("run"):
            run_row, done_row = rows[2], rows[3]
            self.assertIn('<action verb="run"', run_row["completion"])
            self.assertIn('"py_compile"', run_row["completion"])
            self.assertIn(run_row["meta"]["target"], run_row["completion"])
            # The done prompt's transcript carries the real exec_run
            # observation of the verification passing.
            self.assertIn("exit 0", done_row["prompt"])
            self.assertIn('<action verb="done">', done_row["completion"])

    def test_plain_rows_are_unchanged_three_pair_read_patch_done(self):
        for rows in self._tasks_of_shape("plain"):
            self.assertEqual([r["meta"]["pair"] for r in rows], ["read", "patch", "done"])
            self.assertIn("<<<<<<< SEARCH", rows[1]["completion"])

    def test_the_fingerprint_records_the_slice_counts(self):
        self.assertEqual(self.fingerprint["tasks_by_trajectory"], {"find": 4, "plain": 4, "run": 4})


# The scratch directory `flywheel-tool` materializes per request, as it
# appears inside a real `exec_find` hit line. Under ruling bT7/R1
# `Scratch::materialize` names it `flywheel-tool-scratch-{16 hex}`, where the
# hex is a digest of the request's content identity -- so the bytes are the
# same for the same request, on any run, in any process.
_SCRATCH_PATH_RE = re.compile(r"/tmp/flywheel-tool-scratch-[0-9a-f]{16}/")


@unittest.skipUnless(
    REAL_TOOL is not None,
    "flywheel-tool binary not built; run cargo build --release -p bloomery-daemon --bin flywheel-tool",
)
class RealToolDeterminismBoundaryTest(unittest.TestCase):
    """**The determinism law holds end to end** (design spec rule 3: same
    seed -> byte-identical corpus), including the find shape, and this is
    where that is proven against the REAL binary.

    History, because this class used to say the opposite. `exec_find`
    renders each hit as `{canonicalized absolute path}:{lineno}: {line}`,
    and the path it canonicalizes is the throwaway scratch dir the tool
    materializes. The tool originally named that dir from its own PID plus a
    counter, so the three post-`find` rows of every find-shaped task
    (`read`/`patch`/`done` -- the ones carrying the find observation in
    their transcript) differed between two same-seed runs, and
    `corpus_sha256` differed with them: 999 differing rows of 4263 measured.
    This class pinned that as a bounded exception and referred the fix up.

    **Ruling bT7/R1 (2026-08-20) fixed it in the tool, not the factory.**
    `Scratch::materialize` now names the directory from a digest of the
    request's own content identity, so identical requests materialize at
    identical paths. What was explicitly NOT done: no post-processing of
    rendered observation text, and no factory-side rewriting. The find hit
    is still exactly what the real executor emitted, absolute path and all
    -- determinism comes from the executor's input being reproducible, which
    is the only kind of fix that leaves the observation real. That is why
    `test_the_find_rows_still_embed_a_real_absolute_scratch_path` sits
    beside the equality assertion: an "erase the path" regression would
    satisfy determinism and quietly destroy the property determinism was
    protecting.

    `DeterminismTest` in `test_generate.py` cannot see any of this: it
    drives the stub, which materializes nothing. This class drives the real
    binary twice at the same seed and asserts ZERO differing rows."""

    @classmethod
    def setUpClass(cls):
        cls._tmp = tempfile.TemporaryDirectory()
        tmp = Path(cls._tmp.name)
        cls.corpora = []
        for tag in ("a", "b"):
            out, report = tmp / f"{tag}.jsonl", tmp / f"{tag}.json"
            result = run_generate(
                ["--seed", "31", "--count", "6", "--tool", str(REAL_TOOL),
                 "--out", str(out), "--report", str(report)]
            )
            assert result.returncode == 0, result.stderr
            cls.corpora.append(
                [json.loads(line) for line in out.read_text(encoding="utf-8").splitlines()]
            )

    @classmethod
    def tearDownClass(cls):
        cls._tmp.cleanup()

    def test_zero_rows_differ_between_two_runs_at_the_same_seed(self):
        a, b = self.corpora
        self.assertEqual(len(a), len(b))
        differing = [
            (x["meta"]["trajectory"], x["meta"]["pair"]) for x, y in zip(a, b) if x != y
        ]
        self.assertEqual(
            differing,
            [],
            "the determinism law is broken: same seed, different corpus. Before ruling bT7/R1 "
            "this was the find shape's scratch path leaking into the rendered bytes; if it is "
            "that again, check Scratch::materialize's content-derived name.",
        )

    def test_the_find_shape_is_covered_by_that_claim(self):
        # Guards the assertion above from going vacuous: zero differing rows
        # proves nothing about find if no find row was generated.
        shapes = {r["meta"]["trajectory"] for r in self.corpora[0]}
        self.assertIn("find", shapes, f"no find-shaped rows in this corpus: {sorted(shapes)}")

    def test_the_find_rows_still_embed_a_real_absolute_scratch_path(self):
        # Determinism must NOT have been bought by rewriting the observation.
        # `exec_find` emits an absolute canonicalized path per hit; that is
        # real executor output and it must still be there verbatim.
        find_rows = [
            r
            for r in self.corpora[0]
            if r["meta"]["trajectory"] == "find" and r["meta"]["pair"] != "find"
        ]
        self.assertTrue(find_rows, "no post-find rows to check")
        self.assertTrue(
            any(_SCRATCH_PATH_RE.search(r["prompt"]) for r in find_rows),
            "no post-find prompt carries an absolute /tmp/flywheel-tool-scratch-<hex>/ path -- "
            "the find observation is no longer real executor output, or the tool renamed its "
            "scratch dir",
        )


if __name__ == "__main__":
    unittest.main()
