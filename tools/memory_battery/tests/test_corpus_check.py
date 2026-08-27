"""Tests for `tools.memory_battery.corpus_check.check_corpus` (task-2
brief; design spec §3).

Every corpus this file exercises is a small REAL corpus built with Task
1's `generate_corpus` in a tmp dir (seed 1, n=3) -- never a hand-built
fixture -- so these tests prove the checker against the same shape it
will run against for real (the black-oxide rule the design doc cites:
"executed, not asserted").

`CleanCorpusPassesTest` is the positive anchor: a freshly generated,
untouched corpus must pass all four checks. The other four classes are
the brief's falsification invariants, each mutating exactly one thing a
real corpus defect would break and asserting the matching check -- and
only that check -- catches it:

1. `PreFixedDefectFailsCheck1Test` -- the pristine target already carries
   the fix, so the planted test cannot fail-before something that was
   never wrong (check 1).
2. `BrokenReplaceFailsCheck2Test` -- a no-op `replace` (identical to
   `search`) leaves the defect in place after patching, so the planted
   test still fails after the "fix" (check 2).
3. `FlippedByteFailsCheck3Test` -- a single flipped byte in `workspace/`
   (not `pristine/`) desyncs the recomputed sha256 from both the
   manifest value and pristine (check 3).
4. `DoctoredManifestCountFailsCheck4Test` -- the manifest's declared
   `families` dict is incremented independently of the per-task `family`
   fields it is supposed to summarize (check 4).
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any

from tools.memory_battery.corpus import generate_corpus
from tools.memory_battery.corpus_check import check_corpus, main

SEED = 1
N = 3


def _corpus(out_dir: Path) -> dict[str, Any]:
    return generate_corpus(seed=SEED, n=N, out_dir=out_dir)


def _write_manifest(out_dir: Path, manifest: dict[str, Any]) -> None:
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def _read_manifest(out_dir: Path) -> dict[str, Any]:
    return json.loads((out_dir / "manifest.json").read_text(encoding="utf-8"))


def _task_dir(out_dir: Path, entry: dict[str, Any]) -> Path:
    return (out_dir / entry["workspace"]).parent


def _report_failures(report) -> str:
    lines = []
    if not report.families_ok:
        lines.append(f"families: {report.families_detail}")
    for result in report.task_results:
        if not result.ok:
            lines.append(
                f"{result.name}: fails_before={result.fails_before_detail!r} "
                f"passes_after={result.passes_after_detail!r} sha256={result.sha256_detail!r}"
            )
    return "\n".join(lines)


class CleanCorpusPassesTest(unittest.TestCase):
    """A freshly generated, untouched corpus passes every check."""

    def test_clean_corpus_passes_all_checks(self) -> None:
        with TemporaryDirectory() as tmp:
            out_dir = Path(tmp)
            _corpus(out_dir)

            report = check_corpus(out_dir)

            self.assertTrue(report.ok, msg=_report_failures(report))
            self.assertEqual(len(report.task_results), N)
            self.assertTrue(report.families_ok, report.families_detail)
            for result in report.task_results:
                self.assertTrue(result.fails_before_ok, result.fails_before_detail)
                self.assertTrue(result.passes_after_ok, result.passes_after_detail)
                self.assertTrue(result.sha256_ok, result.sha256_detail)


class PreFixedDefectFailsCheck1Test(unittest.TestCase):
    """INVARIANT: a corpus whose defect is pre-fixed (the pristine target
    already carries the fix) fails check 1 -- the planted test cannot
    fail-before something that was never wrong."""

    def test_prefixed_pristine_fails_fails_before_check(self) -> None:
        with TemporaryDirectory() as tmp:
            out_dir = Path(tmp)
            manifest = _corpus(out_dir)
            entry = manifest["tasks"][0]
            target_path = _task_dir(out_dir, entry) / "pristine" / entry["target"]

            content = target_path.read_text(encoding="utf-8")
            self.assertEqual(content.count(entry["search"]), 1)
            target_path.write_text(
                content.replace(entry["search"], entry["replace"], 1), encoding="utf-8"
            )

            report = check_corpus(out_dir)

            self.assertFalse(report.ok)
            result = next(r for r in report.task_results if r.name == entry["name"])
            self.assertFalse(result.fails_before_ok)
            # Editing pristine's bytes also desyncs it from the manifest's
            # recorded workspace_sha256 (and from workspace/) -- check 3
            # correctly fails too; only check 1's failure is asserted here.


class BrokenReplaceFailsCheck2Test(unittest.TestCase):
    """INVARIANT: a corpus whose `replace` breaks the test (a no-op patch
    identical to `search`, leaving the defect in place) fails check 2."""

    def test_noop_replace_fails_passes_after_check(self) -> None:
        with TemporaryDirectory() as tmp:
            out_dir = Path(tmp)
            manifest = _corpus(out_dir)
            entry = manifest["tasks"][0]

            doctored = _read_manifest(out_dir)
            doctored["tasks"][0]["replace"] = doctored["tasks"][0]["search"]
            _write_manifest(out_dir, doctored)

            report = check_corpus(out_dir)

            self.assertFalse(report.ok)
            result = next(r for r in report.task_results if r.name == entry["name"])
            self.assertFalse(result.passes_after_ok)
            # fails-before is read straight from pristine, untouched by this mutation.
            self.assertTrue(result.fails_before_ok, result.fails_before_detail)


class FlippedByteFailsCheck3Test(unittest.TestCase):
    """INVARIANT: a single flipped byte in one task's `workspace/` (not
    `pristine/`) fails check 3 -- the recomputed sha256 desyncs from both
    the manifest value and pristine."""

    def test_flipped_byte_fails_sha256_check(self) -> None:
        with TemporaryDirectory() as tmp:
            out_dir = Path(tmp)
            manifest = _corpus(out_dir)
            entry = manifest["tasks"][0]
            target_path = _task_dir(out_dir, entry) / "workspace" / entry["target"]

            original = target_path.read_bytes()
            # Toggle the ASCII case bit (0x20) of the first letter byte: a
            # genuine single-byte flip that stays valid UTF-8, so the mutation
            # exercises the sha mismatch rather than a decode error.
            idx = next(i for i, b in enumerate(original) if 0x41 <= b <= 0x5A or 0x61 <= b <= 0x7A)
            flipped = bytearray(original)
            flipped[idx] ^= 0x20
            target_path.write_bytes(bytes(flipped))

            report = check_corpus(out_dir)

            self.assertFalse(report.ok)
            result = next(r for r in report.task_results if r.name == entry["name"])
            self.assertFalse(result.sha256_ok)


class DoctoredManifestCountFailsCheck4Test(unittest.TestCase):
    """INVARIANT: a `families` count doctored independently of the
    per-task `family` fields it summarizes fails check 4."""

    def test_doctored_family_count_fails_families_check(self) -> None:
        with TemporaryDirectory() as tmp:
            out_dir = Path(tmp)
            _corpus(out_dir)

            doctored = _read_manifest(out_dir)
            some_family = next(iter(doctored["families"]))
            doctored["families"][some_family] += 1
            _write_manifest(out_dir, doctored)

            report = check_corpus(out_dir)

            self.assertFalse(report.ok)
            self.assertFalse(report.families_ok)


class CliExitCodeTest(unittest.TestCase):
    """The `__main__` CLI exits 0 for a clean corpus and nonzero once the
    manifest is doctored."""

    def test_main_exit_code_tracks_report_ok(self) -> None:
        with TemporaryDirectory() as tmp:
            out_dir = Path(tmp)
            _corpus(out_dir)

            self.assertEqual(main([str(out_dir)]), 0)

            doctored = _read_manifest(out_dir)
            some_family = next(iter(doctored["families"]))
            doctored["families"][some_family] += 1
            _write_manifest(out_dir, doctored)

            self.assertNotEqual(main([str(out_dir)]), 0)


if __name__ == "__main__":
    unittest.main()
