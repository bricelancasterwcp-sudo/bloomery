"""Tests for `tools.memory_battery.corpus.generate_corpus` (task-1 brief;
design spec §3).

Covers the brief's step-1 list verbatim: task-dir materialization
(target + planted `test_file` present), pristine == workspace byte-for-
byte, the pinned manifest schema (top level and per task), same-seed
determinism over the `workspace_sha256` list, seed sensitivity, and every
selected task actually being the run-verified shape (`run_argv` and
`test_file` both non-empty).
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from tools.memory_battery.corpus import generate_corpus

SEED = 1
N = 6

TOP_LEVEL_KEYS = {"instrument", "corpus_seed", "n", "families", "tasks"}
TASK_KEYS = {
    "name",
    "family",
    "workspace",
    "goal",
    "grant",
    "run_argv",
    "search",
    "replace",
    "target",
    "test_file",
    "workspace_sha256",
}
GRANT_KEYS = {"read_roots", "write_roots", "commands"}


def _generate(seed: int, n: int, out_dir: Path) -> dict:
    return generate_corpus(seed=seed, n=n, out_dir=out_dir)


class GenerateCorpusTaskDirsTest(unittest.TestCase):
    """`generate_corpus(seed=1, n=6, tmp)` produces 6 task dirs, each
    containing the target and the planted `test_file`."""

    def test_produces_n_task_dirs_with_target_and_test_file(self) -> None:
        with TemporaryDirectory() as tmp:
            out_dir = Path(tmp)
            manifest = _generate(SEED, N, out_dir)

            self.assertEqual(len(manifest["tasks"]), N)
            task_dirs = sorted((out_dir / "tasks").iterdir())
            self.assertEqual(len(task_dirs), N)

            for entry in manifest["tasks"]:
                workspace_dir = out_dir / entry["workspace"]
                self.assertTrue(workspace_dir.is_dir(), f"missing workspace dir for {entry['name']}")
                target_path = workspace_dir / entry["target"]
                test_path = workspace_dir / entry["test_file"]
                self.assertTrue(target_path.is_file(), f"missing target {target_path}")
                self.assertTrue(test_path.is_file(), f"missing test_file {test_path}")


class PristineMatchesWorkspaceTest(unittest.TestCase):
    """The `pristine/` snapshot is byte-for-byte identical to `workspace/`
    at generation time."""

    def test_pristine_byte_identical_to_workspace(self) -> None:
        with TemporaryDirectory() as tmp:
            out_dir = Path(tmp)
            manifest = _generate(SEED, N, out_dir)

            for entry in manifest["tasks"]:
                task_dir = out_dir / "tasks" / entry["name"]
                workspace_dir = task_dir / "workspace"
                pristine_dir = task_dir / "pristine"

                workspace_files = sorted(p.name for p in workspace_dir.iterdir())
                pristine_files = sorted(p.name for p in pristine_dir.iterdir())
                self.assertEqual(workspace_files, pristine_files)

                for name in workspace_files:
                    self.assertEqual(
                        (workspace_dir / name).read_bytes(),
                        (pristine_dir / name).read_bytes(),
                        f"pristine/{name} diverges from workspace/{name} for {entry['name']}",
                    )


class ManifestSchemaTest(unittest.TestCase):
    """The manifest parses with every pinned field, top level and per
    task, and round-trips through JSON on disk unchanged."""

    def test_manifest_has_every_pinned_field(self) -> None:
        with TemporaryDirectory() as tmp:
            out_dir = Path(tmp)
            manifest = _generate(SEED, N, out_dir)

            self.assertEqual(set(manifest.keys()), TOP_LEVEL_KEYS)
            self.assertEqual(manifest["instrument"], "memory-battery-v1")
            self.assertEqual(manifest["corpus_seed"], SEED)
            self.assertEqual(manifest["n"], N)
            self.assertIsInstance(manifest["families"], dict)
            self.assertEqual(len(manifest["tasks"]), N)

            for entry in manifest["tasks"]:
                self.assertEqual(set(entry.keys()), TASK_KEYS)
                self.assertEqual(set(entry["grant"].keys()), GRANT_KEYS)
                self.assertEqual(entry["grant"]["read_roots"], entry["grant"]["write_roots"])
                self.assertTrue(Path(entry["grant"]["read_roots"][0]).is_absolute())
                self.assertIsInstance(entry["grant"]["commands"], list)
                self.assertIsInstance(entry["workspace_sha256"], str)
                self.assertEqual(len(entry["workspace_sha256"]), 64)  # hex sha256

            on_disk = json.loads((out_dir / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(on_disk, manifest)

    def test_families_counts_equal_observed_task_families(self) -> None:
        """INVARIANT: `families` counts equal the observed per-task
        `family` values -- computed, never declared independently."""
        with TemporaryDirectory() as tmp:
            out_dir = Path(tmp)
            manifest = _generate(SEED, N, out_dir)

            observed: dict[str, int] = {}
            for entry in manifest["tasks"]:
                observed[entry["family"]] = observed.get(entry["family"], 0) + 1

            self.assertEqual(manifest["families"], observed)


class DeterminismTest(unittest.TestCase):
    """INVARIANT: regenerating with the same seed yields byte-identical
    workspaces (via `workspace_sha256`) and manifest content, minus
    absolute-path fields, which derive from `out_dir`. A different seed
    yields a different task list."""

    def test_same_seed_twice_yields_identical_workspace_sha256_list(self) -> None:
        with TemporaryDirectory() as tmp:
            out_dir_a = Path(tmp) / "a"
            out_dir_b = Path(tmp) / "b"
            manifest_a = _generate(SEED, N, out_dir_a)
            manifest_b = _generate(SEED, N, out_dir_b)

            shas_a = [t["workspace_sha256"] for t in manifest_a["tasks"]]
            shas_b = [t["workspace_sha256"] for t in manifest_b["tasks"]]
            self.assertEqual(shas_a, shas_b)

            names_a = [t["name"] for t in manifest_a["tasks"]]
            names_b = [t["name"] for t in manifest_b["tasks"]]
            self.assertEqual(names_a, names_b)
            self.assertEqual(manifest_a["families"], manifest_b["families"])

    def test_different_seed_yields_different_task_list(self) -> None:
        with TemporaryDirectory() as tmp:
            out_dir_a = Path(tmp) / "a"
            out_dir_b = Path(tmp) / "b"
            manifest_a = _generate(SEED, N, out_dir_a)
            manifest_b = _generate(SEED + 1, N, out_dir_b)

            shas_a = [t["workspace_sha256"] for t in manifest_a["tasks"]]
            shas_b = [t["workspace_sha256"] for t in manifest_b["tasks"]]
            self.assertNotEqual(shas_a, shas_b)


class RunVerifiedSelectionTest(unittest.TestCase):
    """Every drawn task's `trajectory` was the run-verified shape: the
    factory rejects a run-verified task with an empty `run_argv` at
    validation time (`task._run_shape_violations`), and the planted-test
    wrapper always sets `test_file` (`templates_run_verified.plant_test`),
    so both must be non-empty for every task the corpus selected."""

    def test_every_task_has_nonempty_run_argv_and_test_file(self) -> None:
        with TemporaryDirectory() as tmp:
            out_dir = Path(tmp)
            manifest = _generate(SEED, N, out_dir)

            for entry in manifest["tasks"]:
                self.assertTrue(entry["run_argv"], f"{entry['name']} has empty run_argv")
                self.assertTrue(entry["test_file"], f"{entry['name']} has empty test_file")
                self.assertIn(("python3", "-m", "unittest"), [tuple(c) for c in entry["grant"]["commands"]])


if __name__ == "__main__":
    unittest.main()
