"""Executing a run-verified task's PLANTED unittest the way the tool will
— and the **fails-before rule** that proves the test can actually fail
(turn-4 spec §3).

**Why turn 4 needed this.** Turn 3's run slice verified with
`python3 -m py_compile <target>`. Every defect the python families plant is
SEMANTIC, so the target compiles before the patch and after it: the
verification could not fail, and the `run` step trained the habit of
verifying without verifying anything. Turn 4 plants a real `unittest`
beside the target instead, and splits the proof that the verification means
something across the two places that can each own half of it:

- **fails-before** is the FACTORY's half, and it lives here: materialize the
  task's files exactly as they ship (UNPATCHED) and run the planted test.
  A nonzero exit is the requirement. A test that passes here is a test the
  ideal's `run` step proves nothing by passing.
- **passes-after** is the TOOL's half (`handle_run_trajectory`): it writes
  the reference-patched target into its scratch dir and runs the same argv
  under the request's grant, refusing to render a trajectory on any nonzero
  exit.

Neither half is redundant and neither can be checked where the other lives:
the factory never sees the patched file the tool builds, and the tool never
sees the unpatched one after the patch lands.

**The child looks like `exec_run`'s child, deliberately.** Same interpreter
resolution (`python3` off `PATH=/usr/bin:/bin`, never `sys.executable` —
the tool will run whatever that PATH resolves to, so the value baked into a
planted test must come from the same interpreter), same fully-rebuilt
environment (`PATH`, `HOME=cwd`, `LANG=C` and nothing else), same
cwd-is-the-workspace shape, stdout and stderr drained into one combined
buffer. A fails-before check run under this process's own environment could
clear a test the tool then fails for an environmental reason, which is the
one way this rule could be locally green and globally wrong.

**Results are cached** because every run here is a pure function of
(files, argv): a deterministic, side-effect-free program over a throwaway
copy of the workspace. The factory draws the same task from the same seed
many times across a test suite, and the same generated corpus re-validates
what it just drew; without the cache each of those repeats pays a fresh
~25ms interpreter start for an answer that cannot have changed.
"""

from __future__ import annotations

import subprocess
import tempfile
from functools import lru_cache
from pathlib import Path
from typing import Sequence

# `exec_run`'s own three environment variables and its fixed PATH
# (`crates/bloomery-daemon/src/task/exec_run.rs`: `RUN_PATH`, `env_clear()`
# then exactly PATH/HOME/LANG).
PYTHON = "python3"
RUN_PATH = "/usr/bin:/bin"

# The argv prefix every run-verified task grants and runs under. Canonical
# HERE, beside the executor that has to agree with it, rather than in the
# template module: `templates_run_verified.py` builds the argv from it,
# `task.py`'s validator matches against it, and the grant line the model
# reads is this prefix space-joined.
UNITTEST_PREFIX: tuple[str, ...] = (PYTHON, "-m", "unittest")

_CACHE_SIZE = 4096


@lru_cache(maxsize=_CACHE_SIZE)
def _run_cached(files: tuple[tuple[str, str], ...], argv: tuple[str, ...]):
    with tempfile.TemporaryDirectory() as tmp:
        workspace = Path(tmp)
        for path, contents in files:
            (workspace / path).write_text(contents, encoding="utf-8")
        return subprocess.run(
            list(argv),
            cwd=workspace,
            env={"PATH": RUN_PATH, "HOME": str(workspace), "LANG": "C"},
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )


def run_python(files: dict[str, str], argv: Sequence[str]) -> subprocess.CompletedProcess:
    """Materializes `files` into a throwaway workspace and runs `argv`
    there under `exec_run`'s environment shape. `.stdout` carries stdout
    and stderr combined, the same single bounded buffer the real executor
    reports as an observation's content.

    Every file is written at the workspace root: the factory's workspaces
    are flat (the tool's own `Scratch` materializes them the same way), so
    a nested path here would be a bug in the caller rather than something
    to silently create directories for."""
    return _run_cached(tuple(sorted(files.items())), tuple(argv))


def fails_before_violations(files: dict[str, str], test_file: str, run_argv: Sequence[str]) -> list[str]:
    """The fails-before rule, as a list of human-readable violations (empty
    means clean) — `task._run_shape_violations`'s expensive branch.

    The three cheap structural checks come first and each returns early,
    for one reason: they are the conditions under which EXECUTING the test
    would answer a different question than the one being asked. Without a
    `test_file` there is nothing to run; with one absent from `files` the
    run would fail for a missing-import reason rather than a defect reason
    (a nonzero exit that would pass this rule for entirely the wrong
    cause); and with a `run_argv` that never names the planted test, the
    factory would be clearing one command while the tool executes another."""
    if not test_file:
        return [
            "run-verified task has an empty test_file -- the run step must execute a planted "
            "test, and a task carrying none has no verification to prove can fail"
        ]

    if test_file not in files:
        return [
            f"test_file {test_file!r} is not among the task's files {sorted(files)} -- the "
            f"planted test must ship with the workspace it verifies"
        ]

    if test_file not in tuple(run_argv):
        return [
            f"run_argv {list(run_argv)} does not name the planted test {test_file!r} -- the "
            f"fails-before proof and the tool's passes-after run must be about the same command"
        ]

    result = run_python(files, run_argv)
    if result.returncode == 0:
        return [
            f"the planted test {test_file!r} passes against the unpatched workspace (exit 0) -- "
            f"a verification that cannot fail proves nothing when it passes after the patch, "
            f"which is exactly the turn-3 py_compile failure this rule exists to prevent.\n"
            f"{result.stdout}"
        ]
    return []
