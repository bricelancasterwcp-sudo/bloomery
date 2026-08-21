"""envelope-v4: the grant line the factory's corpus is now rendered under
(turn-4 spec §1/§2).

**Why the envelope moved.** Through v3 a rendered prompt was
`goal + verb card + transcript` and never mentioned the grant, so a
run-granted task and a plain one were token-identical at the moment that
decides everything — the step right after a successful `patch` observation.
Turn 3's corpus voted `done` 666 : `run` 333 on those indistinguishable
inputs, supervised fine-tuning took the majority, and the trained model
emitted **zero** `run` verbs at probe time. v4 adds one line, rendered from
the `Grant` the loop enforces, and that line is the only thing dissolving
the conflict — so its exact bytes, on every shape, are what this file pins.

Split out of `test_generate_trajectories.py` when it pushed that file past
the 400-line house cap; the seam is the subject, not the size. That file
owns the three repair SHAPES (which slot renders which, how many pairs each
returns, and the determinism boundary); this one owns the ENVELOPE those
shapes are rendered under, and it is the only place either grant-line form
is written down. Its `_generate`/`_rows_by_task`/`REAL_TOOL`/`STUB_TOOL`
helpers are imported from there rather than re-declared.
"""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from tools.flywheel.factory import generate_request, templates
from tools.flywheel.factory.task import Task
from tools.flywheel.tests.test_generate import REAL_TOOL, STUB_TOOL
from tools.flywheel.tests.test_generate_trajectories import _generate, _rows_by_task

# The grant line, exactly as `task::grant_line` renders it. The granted
# form is derived from the factory's own prefix so a change to the grant
# cannot leave this expectation behind; the `none` form is a literal
# because its bytes — including the em dash, U+2014 — are the contract, and
# a derived copy would just be a second place to get them wrong.
GRANTED_LINE = "Granted commands: " + " ".join(templates.UNITTEST_PREFIX)
NONE_LINE = "Granted commands: none — run is not available in this task"

CONTENTS = "def add(a, b):\n    return a + b\n\n\ndef sub(a, b):\n    return a - b\n"


def _task(**extra):
    return Task(
        name="fam",
        lens="python",
        target="mathy.py",
        files={"mathy.py": CONTENTS},
        goal="mathy.py's add() is broken. Patch the file, then emit done.",
        search="    return a + b",
        replace="    return a + b  # ok",
        summary="Fixed add().",
        **extra,
    )


RUN_FIELDS = dict(
    trajectory="run",
    run_argv=("python3", "-m", "unittest", "test_mathy.py"),
    commands=(("python3", "-m", "unittest"),),
    test_file="test_mathy.py",
)


class RequestsCarryTheEnvelopeAndGrantTest(unittest.TestCase):
    """Every request the factory sends declares BOTH, on every shape: the
    envelope every turn-4 measurement is made under, and the grant the
    prompt will state. `commands: []` is sent explicitly rather than
    omitted — an absent key and an empty list mean the same thing to the
    tool's deserializer, but only the explicit form says the factory
    decided, and this is a field whose value becomes prompt text."""

    def test_every_shape_declares_envelope_v4_and_a_commands_list(self):
        shapes = {
            "plain": _task(),
            "find": _task(trajectory="find", find_pattern="def add"),
            "run": _task(**RUN_FIELDS),
        }
        expected_commands = {"plain": [], "find": [], "run": [["python3", "-m", "unittest"]]}
        for shape, task in shapes.items():
            request = generate_request.build_trajectory_request(task)
            self.assertEqual(request["envelope"], "v4", shape)
            self.assertEqual(request["commands"], expected_commands[shape], shape)


class StubSpeaksTheSameEnvelopeAsTheRealToolTest(unittest.TestCase):
    """The stub is what every unit-level generate test sees, so a stub that
    quietly ignored `envelope`/`commands` would let the whole suite pass
    against a prompt shape the real binary never renders. It therefore
    parses the envelope, renders the same grant line, and refuses the same
    unusable argv prefixes the real tool names as errors."""

    def _ask(self, request):
        proc = subprocess.run(
            [sys.executable, str(STUB_TOOL)],
            input=json.dumps(request) + "\n",
            capture_output=True,
            text=True,
        )
        return json.loads(proc.stdout.splitlines()[0])

    def test_a_plain_request_renders_the_none_line(self):
        response = self._ask(generate_request.build_trajectory_request(_task()))
        for pair in response["pairs"]:
            self.assertIn(NONE_LINE, pair["prompt"])

    def test_a_run_request_renders_one_granted_line_per_prefix(self):
        response = self._ask(generate_request.build_trajectory_request(_task(**RUN_FIELDS)))
        for pair in response["pairs"]:
            self.assertIn(GRANTED_LINE, pair["prompt"])
            self.assertNotIn(NONE_LINE, pair["prompt"])

    def test_an_unknown_envelope_is_a_named_error(self):
        request = generate_request.build_trajectory_request(_task())
        request["envelope"] = "v9"
        self.assertIn("envelope", self._ask(request)["error"])

    def test_an_empty_granted_prefix_is_a_named_error(self):
        request = generate_request.build_trajectory_request(_task())
        request["commands"] = [[]]
        self.assertIn("commands", self._ask(request)["error"])

    def test_a_blank_word_in_a_granted_prefix_is_a_named_error(self):
        request = generate_request.build_trajectory_request(_task())
        request["commands"] = [["python3", "  "]]
        self.assertIn("commands", self._ask(request)["error"])


@unittest.skipUnless(
    REAL_TOOL is not None,
    "flywheel-tool binary not built; run cargo build --release -p bloomery-daemon --bin flywheel-tool",
)
class RealToolGrantLineTest(unittest.TestCase):
    """The authority: the grant line as the REAL binary renders it, over a
    corpus carrying all three repair shapes and both refusal classes. The
    stub can only echo what it was told; only this proves the line the model
    will read comes from the same renderer the task loop uses."""

    @classmethod
    def setUpClass(cls):
        cls._tmp = tempfile.TemporaryDirectory()
        tmp = Path(cls._tmp.name)
        result, out, _report = _generate(
            tmp,
            ["--seed", "5", "--count", "12", "--refusal-count", "6", "--tool", str(REAL_TOOL)],
        )
        assert result.returncode == 0, result.stderr
        cls.by_task = _rows_by_task(out)

    @classmethod
    def tearDownClass(cls):
        cls._tmp.cleanup()

    def _rows_of(self, predicate, label):
        rows = [rows for rows in self.by_task.values() if predicate(rows[0]["meta"])]
        self.assertTrue(rows, f"no {label} task in the run")
        return rows

    def _of_shape(self, shape):
        return self._rows_of(lambda meta: meta.get("trajectory") == shape, shape)

    def test_run_shaped_prompts_open_with_the_granted_line(self):
        """The whole point of turn 4: the decision point after a successful
        patch is no longer token-identical between a run-granted task and a
        plain one. `startswith` rather than `in`, because the line's
        POSITION is part of the contract — `goal`, blank line, grant, blank
        line, verb card."""
        for rows in self._of_shape("run"):
            for row in rows:
                self.assertTrue(
                    row["prompt"].startswith(f"{row['meta']['goal']}\n\n{GRANTED_LINE}\n\n"),
                    f"{row['meta']['task_id']}/{row['meta']['pair']}: {row['prompt'][:200]!r}",
                )

    def test_plain_and_find_prompts_carry_the_none_line(self):
        for shape in ("plain", "find"):
            for rows in self._of_shape(shape):
                for row in rows:
                    self.assertTrue(
                        row["prompt"].startswith(f"{row['meta']['goal']}\n\n{NONE_LINE}\n\n"),
                        f"{row['meta']['task_id']}/{shape}: {row['prompt'][:200]!r}",
                    )
                    self.assertNotIn(GRANTED_LINE, row["prompt"])

    def test_refuse_prompts_carry_the_none_line_and_still_render_two_pairs(self):
        # A refuse task grants nothing by construction (`RefusalTask` has no
        # commands field at all), so its `none` line is structural rather
        # than defaulted -- and the pair count must not have moved.
        for rows in self._rows_of(lambda meta: meta.get("expect") == "refuse", "refuse"):
            self.assertEqual([r["meta"]["pair"] for r in rows], ["read", "done"])
            for row in rows:
                self.assertIn(NONE_LINE, row["prompt"])
                self.assertNotIn(GRANTED_LINE, row["prompt"])


if __name__ == "__main__":
    unittest.main()
