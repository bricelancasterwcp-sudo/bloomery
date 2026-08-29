"""Tests for `tools.memory_battery.dry_manifest.generate_run_manifest` and
its CLI (`main`) -- the controller-ruled preliminary to task-3's
pre-registration lock (task-2's shakedown found the real 50-task run
needs the identical scratch-copy + grant-rewrite mechanics as the 3-task
dry shakedown, plus a `--real` mode that drives the manifest's FULL task
list rather than a caller-chosen subset).

Every corpus this file exercises is a small REAL corpus built with
`corpus.generate_corpus` in a tmp dir -- never the tracked `corpus-v1/`
tree -- the same convention `test_corpus_check.py` uses, so these tests
never touch, and cannot corrupt, the actual frozen instrument. `N = 5`
(greater than `DRY_TASK_COUNT = 3`) so dry and real modes produce
genuinely different subset sizes and the distinction is actually
exercised, not merely asserted past a case where both modes would
coincide.
"""

from __future__ import annotations

import hashlib
import io
import json
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from tempfile import TemporaryDirectory

from tools.memory_battery.corpus import generate_corpus
from tools.memory_battery.dry_manifest import DRY_TASK_COUNT, generate_run_manifest, main

SEED = 7
N = 5


def _corpus(out_dir: Path) -> dict:
    return generate_corpus(seed=SEED, n=N, out_dir=out_dir)


def _sha_tree(root: Path) -> dict[str, str]:
    """sha256 of every file under `root`, keyed by path relative to
    `root` -- a git-status-style fingerprint of the tree's bytes, used to
    prove the frozen corpus is untouched by construction (before/after
    equality is the check; a single mutated or missing/added file changes
    this dict)."""
    digests: dict[str, str] = {}
    for path in sorted(root.rglob("*")):
        if path.is_file():
            digests[str(path.relative_to(root))] = hashlib.sha256(path.read_bytes()).hexdigest()
    return digests


class DryDefaultBehaviorTests(unittest.TestCase):
    """Dry mode (no --real) keeps its pre-existing, pinned behavior:
    first DRY_TASK_COUNT tasks in manifest order, "dry": true,
    "scratch_copy": true, grants rewritten onto the scratch copy."""

    def test_default_count_and_dry_stamp(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            corpus_dir = tmp / "corpus"
            manifest = _corpus(corpus_dir)
            out_path = tmp / "dry" / "manifest.json"

            result = generate_run_manifest(corpus_dir, out_path)

            self.assertEqual(len(result["tasks"]), DRY_TASK_COUNT)
            self.assertEqual(result["n"], DRY_TASK_COUNT)
            self.assertIs(result["dry"], True)
            self.assertIs(result["scratch_copy"], True)
            expected_names = [t["name"] for t in manifest["tasks"][:DRY_TASK_COUNT]]
            self.assertEqual([t["name"] for t in result["tasks"]], expected_names)

            expected_prefix = str((out_path.parent / "tasks").resolve())
            corpus_prefix = str(corpus_dir.resolve())
            for task in result["tasks"]:
                for root_key in ("read_roots", "write_roots"):
                    (root_path,) = task["grant"][root_key]
                    self.assertTrue(root_path.startswith(expected_prefix), root_path)
                    self.assertNotIn(corpus_prefix, root_path)

            on_disk = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual(on_disk, result)

    def test_explicit_count_still_honored_in_dry_mode(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            corpus_dir = tmp / "corpus"
            _corpus(corpus_dir)
            out_path = tmp / "dry" / "manifest.json"

            result = generate_run_manifest(corpus_dir, out_path, count=2)

            self.assertEqual(len(result["tasks"]), 2)
            self.assertEqual(result["n"], 2)
            self.assertIs(result["dry"], True)


class RealModeTests(unittest.TestCase):
    """--real / real=True: full task count (matching the frozen
    manifest's own `n`), no "dry" stamp, the same scratch-copy grant
    mechanics as dry mode."""

    def test_real_uses_full_task_list_and_omits_dry_stamp(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            corpus_dir = tmp / "corpus"
            manifest = _corpus(corpus_dir)
            out_path = tmp / "real" / "manifest.json"

            result = generate_run_manifest(corpus_dir, out_path, real=True)

            self.assertEqual(len(result["tasks"]), N)
            self.assertEqual(result["n"], N)
            self.assertNotIn("dry", result)
            self.assertIs(result["scratch_copy"], True)
            self.assertEqual(
                [t["name"] for t in result["tasks"]],
                [t["name"] for t in manifest["tasks"]],
            )

            expected_prefix = str((out_path.parent / "tasks").resolve())
            corpus_prefix = str(corpus_dir.resolve())
            for task in result["tasks"]:
                for root_key in ("read_roots", "write_roots"):
                    (root_path,) = task["grant"][root_key]
                    self.assertTrue(root_path.startswith(expected_prefix), root_path)
                    self.assertNotIn(corpus_prefix, root_path)

            on_disk = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual(on_disk, result)
            self.assertNotIn("dry", on_disk)

    def test_real_and_explicit_count_rejected_at_library_level(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            corpus_dir = tmp / "corpus"
            _corpus(corpus_dir)
            out_path = tmp / "real" / "manifest.json"

            with self.assertRaises(ValueError) as ctx:
                generate_run_manifest(corpus_dir, out_path, count=2, real=True)
            self.assertIn("--count", str(ctx.exception))
            self.assertIn("--real", str(ctx.exception))
            self.assertFalse(out_path.exists())  # rejected before any write

    def test_real_and_explicit_count_rejected_at_cli_level(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            corpus_dir = tmp / "corpus"
            _corpus(corpus_dir)
            out_path = tmp / "real" / "manifest.json"

            argv = [
                "--corpus-dir", str(corpus_dir),
                "--out", str(out_path),
                "--real",
                "--count", "2",
            ]
            with self.assertRaises(SystemExit) as ctx:
                main(argv)
            self.assertEqual(ctx.exception.code, 2)
            self.assertFalse(out_path.exists())


class FrozenTreeUntouchedTests(unittest.TestCase):
    """Both modes open `corpus_dir` READ-ONLY -- the frozen tree's own
    bytes are unchanged before vs. after, in both dry and real mode --
    and the scratch copy is a genuine copy, not the same file (mutating
    the scratch copy afterward never touches the source)."""

    def test_real_run_leaves_frozen_tree_byte_identical(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            corpus_dir = tmp / "corpus"
            _corpus(corpus_dir)
            before = _sha_tree(corpus_dir)
            self.assertGreater(len(before), 0)

            out_path = tmp / "real" / "manifest.json"
            generate_run_manifest(corpus_dir, out_path, real=True)

            after = _sha_tree(corpus_dir)
            self.assertEqual(before, after)

    def test_dry_run_leaves_frozen_tree_byte_identical(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            corpus_dir = tmp / "corpus"
            _corpus(corpus_dir)
            before = _sha_tree(corpus_dir)

            out_path = tmp / "dry" / "manifest.json"
            generate_run_manifest(corpus_dir, out_path)

            after = _sha_tree(corpus_dir)
            self.assertEqual(before, after)

    def test_scratch_copy_is_independent_of_source(self) -> None:
        # Mutating the scratch copy's file afterward must never touch the
        # frozen tree's own bytes -- proves copytree, not a symlink/alias
        # (the exact failure mode the task-2 shakedown hit and fixed).
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            corpus_dir = tmp / "corpus"
            _corpus(corpus_dir)
            before = _sha_tree(corpus_dir)

            out_path = tmp / "real" / "manifest.json"
            result = generate_run_manifest(corpus_dir, out_path, real=True)

            first_task = result["tasks"][0]
            (scratch_workspace,) = first_task["grant"]["write_roots"]
            scratch_files = [p for p in Path(scratch_workspace).glob("*") if p.is_file()]
            self.assertTrue(scratch_files)
            scratch_files[0].write_text("mutated by test\n", encoding="utf-8")

            after = _sha_tree(corpus_dir)
            self.assertEqual(before, after)

    def test_repeated_generation_still_leaves_frozen_tree_byte_identical(self) -> None:
        # generate_run_manifest is documented safe to call repeatedly
        # (each task's scratch pair is rmtree'd + recopied) -- confirm
        # that repetition never drifts into touching the source either.
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            corpus_dir = tmp / "corpus"
            _corpus(corpus_dir)
            before = _sha_tree(corpus_dir)

            out_path = tmp / "real" / "manifest.json"
            generate_run_manifest(corpus_dir, out_path, real=True)
            generate_run_manifest(corpus_dir, out_path, real=True)

            after = _sha_tree(corpus_dir)
            self.assertEqual(before, after)


class CliTests(unittest.TestCase):
    def test_cli_dry_default_reports_dry_mode(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            corpus_dir = tmp / "corpus"
            _corpus(corpus_dir)
            out_path = tmp / "dry" / "manifest.json"

            stdout = io.StringIO()
            with redirect_stdout(stdout):
                exit_code = main(["--corpus-dir", str(corpus_dir), "--out", str(out_path)])

            self.assertEqual(exit_code, 0)
            self.assertIn("(dry mode)", stdout.getvalue())
            written = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual(len(written["tasks"]), DRY_TASK_COUNT)
            self.assertIs(written["dry"], True)

    def test_cli_real_reports_real_mode_and_full_count(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            corpus_dir = tmp / "corpus"
            _corpus(corpus_dir)
            out_path = tmp / "real" / "manifest.json"

            stdout = io.StringIO()
            with redirect_stdout(stdout):
                exit_code = main(["--corpus-dir", str(corpus_dir), "--out", str(out_path), "--real"])

            self.assertEqual(exit_code, 0)
            self.assertIn("(real mode)", stdout.getvalue())
            written = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual(len(written["tasks"]), N)
            self.assertNotIn("dry", written)
            self.assertIs(written["scratch_copy"], True)


if __name__ == "__main__":
    unittest.main()


class PristineP2CarryTests(unittest.TestCase):
    """premise-gone-battery plan Task 5: a task carrying `pristine_p2`
    gets that tree scratch-copied beside `workspace/`+`pristine/`, and
    the output manifest's key names the scratch copy; tasks without the
    key are untouched (the rest of this file, unmodified, pins that)."""

    def test_pristine_p2_is_scratch_copied_and_the_key_rewritten(self) -> None:
        from tools.memory_battery.corpus_pg import generate_corpus_pg

        with TemporaryDirectory() as tmp:
            corpus_dir = Path(tmp) / "corpus"
            generate_corpus_pg(1, 2, corpus_dir)
            out_path = Path(tmp) / "run" / "manifest.json"

            frozen_before = _sha_tree(corpus_dir)
            run_manifest = generate_run_manifest(corpus_dir, out_path, real=True)
            self.assertEqual(_sha_tree(corpus_dir), frozen_before)

            for task in run_manifest["tasks"]:
                scratch_p2 = out_path.parent / task["pristine_p2"]
                frozen_p2 = corpus_dir / "tasks" / task["name"] / "pristine_p2"
                self.assertTrue(scratch_p2.is_dir())
                # Sibling convention the driver resolves by.
                self.assertEqual(scratch_p2.parent.name, task["name"])
                self.assertEqual(scratch_p2.name, "pristine_p2")
                self.assertEqual(_sha_tree(scratch_p2), _sha_tree(frozen_p2))
                # The scratch p2 tree is outside the frozen corpus.
                self.assertNotIn(str(corpus_dir), str(scratch_p2.resolve()))
