"""Turn 4's run-verified slice: each of the eight python families wrapped
so its task ships a PLANTED `unittest` that fails before the reference
patch and passes after it (turn-4 spec §3).

Split out of `templates_python.py` when the PROBE table and the
expected-value machinery pushed that module past the 400-line house cap —
the same split turn 1 made for `templates_python.py`/`templates_text.py`.
The seam is exact: `templates_python.py` owns the eight DEFECTS,
this module owns the VERIFICATION built around them.

**Still a wrapper, never a copy.** Every family is wrapped rather than
re-authored, so an edit to a defect family propagates to its run-verified
twin for free; `test_templates_multifile.py` re-derives each wrapper's body
from its base family at the same seed, which is what would catch a
copy-pasted variant drifting.

**What the wrapper plants.** For a task on `<stem>.py` it adds
`test_<stem>.py` to `files` — a one-method `unittest.TestCase` that imports
the target by stem and asserts the goal's expected behavior — and sets
`run_argv = ("python3", "-m", "unittest", "test_<stem>.py")` with
`commands = (("python3", "-m", "unittest"),)`. The grant states the argv
PREFIX, so the line the model reads under envelope-v4 is
`Granted commands: python3 -m unittest`.

**The expected value is never hand-typed.** It is computed at generation
time by applying the family's own search->replace to the target and
executing the resulting REFERENCE-PATCHED module in a subprocess, then
calling the family's probe and taking `repr` of what came back. A
hand-typed expectation is a second statement of what the fix does, free to
disagree with the fix itself; a computed one cannot. If the probe raises or
the patched module does not import, that is a factory bug and it aborts
loudly here rather than shipping a test nobody can run.

**The probe is one call, deliberately.** The planted test's job is to
distinguish patched from unpatched, not to characterize the function: one
assertion keeps the ideal's `run` observation short (it becomes prompt text
for the `done` step) and keeps the fails-before rule's answer unambiguous.
"""

from __future__ import annotations

import random
import re
from typing import Callable, NamedTuple

from tools.flywheel.factory import planted_test, templates_python
from tools.flywheel.factory.planted_test import UNITTEST_PREFIX
from tools.flywheel.factory.task import RUN_TRAJECTORY, Task
from tools.flywheel.factory.wordlists import DICT_KEY_POOL, FLAG_NAMES

RUN_FAMILY_SUFFIX = "_run_verified"


class Probe(NamedTuple):
    """How a family's planted test calls the function carrying the defect.

    `func` is a regex matching that function's NAME on its `def` line, not
    the name itself: every family builds its identifiers from rng-drawn
    theme nouns, so the name is only knowable per draw. The first `def` in
    the file whose name matches is the probed one — true for all eight
    families, since each plants its defect in the file's opening function.
    Naming a per-family pattern rather than "the first def" is what makes a
    family that later moves its defect fail loudly here instead of silently
    probing the wrong function.

    `args` is the call's argument list as source text, spliced verbatim
    into the planted test. It is chosen so the buggy and fixed functions
    return DIFFERENT values — that difference is the whole content of the
    verification, and the fails-before rule (`planted_test.py`) is what
    proves it holds for every draw rather than for the one that was tried
    by hand."""

    func: str
    args: str


# The dict-key family's probe argument: every key in the pool, each mapped
# to a distinct value. Built from `DICT_KEY_POOL` rather than written out
# because the family draws its correct and wrong keys FROM that pool — an
# entry naming only some of the keys would raise `KeyError` on the draws it
# missed, and a hand-written copy would rot the moment the pool changed.
# Distinct values are what make "read the wrong key" observable at all.
_ENTRY_LITERAL = repr({key: index + 1 for index, key in enumerate(DICT_KEY_POOL)})

# One entry per family in `templates_python.FAMILIES` — asserted
# mechanically in `test_templates_multifile.py`, since a missing entry
# would be a family that cannot plant a test and a stale one would be a
# probe nothing exercises.
PROBE: dict[str, Probe] = {
    # keeps the extreme of the list: buggy `<` keeps 1, fixed `>` keeps 5
    # (and the mirror draw, which fixes to `<`, inverts both).
    "py_wrong_comparison_operator": Probe(r"(?:highest|lowest)_\w+", "[3, 1, 4, 1, 5]"),
    # buggy returns (7, 7); fixed returns (7, 9).
    "py_off_by_one_index": Probe(r"first_and_last_\w+", "[7, 8, 9]"),
    # buggy yields 2 markers for a count of 3; fixed yields 3.
    "py_off_by_one_range_bound": Probe(r"\w+_checkpoints", "3"),
    # every factor in the family's pool gives a distinct product with 8.
    "py_wrong_constant_multiplier": Probe(r"scaled_\w+", "8"),
    # the threshold is drawn from 10..90, so 0 always fails the first
    # condition and the connector alone decides: `and` -> False, `or` -> True.
    "py_inverted_boolean": Probe(r"(?:%s)" % "|".join(FLAG_NAMES), "0, True"),
    # buggy doubles the first argument twice (12); fixed combines both (16).
    "py_wrong_variable_reference": Probe(r"combined_\w+", "3, 5"),
    # buggy renders the first value in both fields; fixed renders both.
    "py_wrong_fstring_field": Probe(r"describe_\w+", "3, 5"),
    # buggy reads the wrong key out of the entry; fixed reads the right one.
    "py_wrong_dict_key": Probe(r"\w+_value", _ENTRY_LITERAL),
}


def probed_function(contents: str, probe: Probe) -> str:
    """The name of the function `probe` selects out of `contents`: the
    FIRST top-level `def` whose name matches `probe.func`.

    A family whose file no longer defines a matching function is a factory
    bug — the planted test would not compile and the fails-before rule
    would then pass for the wrong reason (a nonzero exit caused by a
    NameError rather than by the defect) — so it raises rather than
    returning a fallback."""
    match = re.search(rf"^def ({probe.func})\(", contents, re.MULTILINE)
    if match is None:
        raise ValueError(
            f"probe {probe.func!r} matches no top-level function in the generated target; "
            f"the family and its probe have drifted apart:\n{contents}"
        )
    return match.group(1)


def _expected_repr(patched_contents: str, stem: str, func: str, args: str) -> str:
    """`repr(<stem>.<func>(<args>))` as computed by executing the
    REFERENCE-PATCHED module — the value the planted test asserts."""
    program = f"import {stem}\nprint(repr({stem}.{func}({args})), end='')\n"
    result = planted_test.run_python(
        {f"{stem}.py": patched_contents}, (planted_test.PYTHON, "-c", program)
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"could not compute the expected value for {stem}.{func}({args}) against the "
            f"reference-patched source (exit {result.returncode}):\n{result.stdout}"
        )
    return result.stdout


def _test_source(stem: str, func: str, args: str, expected: str) -> str:
    """The planted test file, verbatim. Ordinary hand-written shape (stdlib
    import, module import, one `TestCase`, the `__main__` guard) because it
    ships as training text and as a real gate-fixture sibling: a file that
    read as machine output would teach the model that verification looks
    like machine output."""
    return (
        f"import unittest\n"
        f"\n"
        f"import {stem}\n"
        f"\n"
        f"\n"
        f"class Test{stem.capitalize()}(unittest.TestCase):\n"
        f"    def test_{func}(self):\n"
        f"        self.assertEqual({stem}.{func}({args}), {expected})\n"
        f"\n"
        f"\n"
        f'if __name__ == "__main__":\n'
        f"    unittest.main()\n"
    )


def plant_test(task: Task, probe: Probe) -> Task:
    """Returns `task` with its planted test, run argv and grant attached.
    Public so a gate-fixture author can plant the same test the corpus
    plants, from the same code, rather than by transcription."""
    contents = task.files[task.target]
    stem = task.target.removesuffix(".py")
    test_file = f"test_{stem}.py"
    func = probed_function(contents, probe)
    expected = _expected_repr(contents.replace(task.search, task.replace, 1), stem, func, probe.args)

    files = dict(task.files)
    files[test_file] = _test_source(stem, func, probe.args, expected)
    return task._replace(
        name=f"{task.name}{RUN_FAMILY_SUFFIX}",
        files=files,
        trajectory=RUN_TRAJECTORY,
        run_argv=UNITTEST_PREFIX + (test_file,),
        commands=(UNITTEST_PREFIX,),
        test_file=test_file,
    )


def _run_verified(probe: Probe, fn: Callable[[random.Random], Task]) -> Callable[[random.Random], Task]:
    """Wraps a plain family so its task renders the run-verified shape.
    Draws through to `fn` unchanged, so the wrapper consumes exactly the
    same rng sequence the base family does -- determinism (rule 3) is
    inherited, not re-established. The planted test consumes no rng at all:
    every part of it is a function of the drawn task plus the family's
    fixed probe."""

    def wrapper(rng: random.Random) -> Task:
        return plant_test(fn(rng), probe)

    return wrapper


RUN_FAMILIES = {
    f"{name}{RUN_FAMILY_SUFFIX}": _run_verified(PROBE[name], fn)
    for name, fn in templates_python.FAMILIES.items()
}
