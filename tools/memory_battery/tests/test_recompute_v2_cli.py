"""CLI-layer enforcement tests for `tools.memory_battery.recompute_v2`
(review findings CRITICAL + IMPORTANT-1, `.superpowers/sdd/
2026-08-28-refalsify-battery-v2/task-1-report.md`'s fix-report section).

Split out of `test_recompute_v2.py` to keep both files under the house
800-line ceiling (`coding-style.md`) once these review-driven additions
(digest-mismatch/completeness FATAL enforcement) grew the CLI coverage --
reuses that module's fixture builders directly rather than duplicating
~280 lines of journal/ledger-writing helpers.

`completeness` (a v2 port of v1's `_check_arm_completeness`/C2) is tested
here alongside the CLI, since its only externally-observable ENFORCEMENT
is at the CLI layer (the library `recompute_v2()` stays permissive -- see
`recompute_v2.py`'s own module docstring); `recompute_v2.py`'s output-key
correctness for `completeness` itself is exercised by these same tests.
"""

from __future__ import annotations

import contextlib
import io
import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from tools.memory_battery.recompute_v2 import main, recompute_v2
from tools.memory_battery.tests.test_recompute_v2 import CONSTANT_50, TASKS, _build_fixture


class CompletenessTests(unittest.TestCase):
    """A dedicated fixture (not the dropped-task fixture used elsewhere) --
    a truncated arm's ledger carries fewer than `2*n` task-half rows, and
    `completeness` must name that fact explicitly, independent of H3's
    infra-rate ceiling."""

    def test_completeness_pass_when_both_arms_full(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(tmp, TASKS, CONSTANT_50, CONSTANT_50, CONSTANT_50, CONSTANT_50)
            result = recompute_v2(**paths)
            completeness = result["completeness"]
            self.assertFalse(completeness["violated"])
            self.assertFalse(completeness["m_prime"]["violated"])
            self.assertFalse(completeness["r"]["violated"])
            self.assertEqual(completeness["m_prime"]["actual_task_halves"], 12)
            self.assertEqual(completeness["m_prime"]["expected_task_halves"], 12)

    def test_completeness_violation_on_truncated_arm(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            # A missing p2 ledger row for one R task -- 11 task-half rows,
            # not the expected 2*n=12 -- an incomplete arm, independent of
            # whatever H3's infra-rate ceiling separately says about it.
            paths = _build_fixture(
                tmp, TASKS, CONSTANT_50, CONSTANT_50, CONSTANT_50, CONSTANT_50, r_skip_p2={"t3"}
            )
            result = recompute_v2(**paths)
            completeness = result["completeness"]
            self.assertTrue(completeness["violated"])
            self.assertFalse(completeness["m_prime"]["violated"])
            r_completeness = completeness["r"]
            self.assertTrue(r_completeness["violated"])
            self.assertEqual(r_completeness["actual_task_halves"], 11)
            self.assertEqual(r_completeness["expected_task_halves"], 12)
            self.assertIn("truncated", r_completeness["reason"])


class CliTests(unittest.TestCase):
    def test_cli_requires_expected_digest(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(tmp, TASKS, CONSTANT_50, CONSTANT_50, CONSTANT_50, CONSTANT_50)
            argv = [
                "--corpus-dir", str(paths["corpus_dir"]),
                "--arm-m-prime-dir", str(paths["arm_m_prime_dir"]),
                "--arm-r-dir", str(paths["arm_r_dir"]),
                "--ledger-m-prime", str(paths["ledger_m_prime"]),
                "--ledger-r", str(paths["ledger_r"]),
            ]
            with self.assertRaises(SystemExit):
                main(argv)

    def test_cli_fatal_and_nonzero_on_digest_mismatch(self) -> None:
        # `_build_fixture`'s default digests are "digest-m-prime" (M') vs
        # "digest-r" (R) -- passing "digest-m-prime" matches M' but
        # MISMATCHES R. Review finding CRITICAL: either arm's mismatch
        # must FATAL the whole run, never print JSON, and exit nonzero.
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(tmp, TASKS, CONSTANT_50, CONSTANT_50, CONSTANT_50, CONSTANT_50)
            argv = [
                "--corpus-dir", str(paths["corpus_dir"]),
                "--arm-m-prime-dir", str(paths["arm_m_prime_dir"]),
                "--arm-r-dir", str(paths["arm_r_dir"]),
                "--ledger-m-prime", str(paths["ledger_m_prime"]),
                "--ledger-r", str(paths["ledger_r"]),
                "--expected-digest", "digest-m-prime",
            ]
            stdout = io.StringIO()
            stderr = io.StringIO()
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                exit_code = main(argv)
            self.assertNotEqual(exit_code, 0)
            self.assertEqual(stdout.getvalue(), "")  # no JSON printed on a FATAL
            self.assertIn("FATAL", stderr.getvalue())
            self.assertIn("identity mismatch", stderr.getvalue())
            self.assertIn("'r'", stderr.getvalue())

    def test_cli_prints_json_when_digest_matches_both_arms(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(
                tmp,
                TASKS,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                digest_m_prime=("shared-digest", "shared-digest"),
                digest_r=("shared-digest", "shared-digest"),
            )
            argv = [
                "--corpus-dir", str(paths["corpus_dir"]),
                "--arm-m-prime-dir", str(paths["arm_m_prime_dir"]),
                "--arm-r-dir", str(paths["arm_r_dir"]),
                "--ledger-m-prime", str(paths["ledger_m_prime"]),
                "--ledger-r", str(paths["ledger_r"]),
                "--expected-digest", "shared-digest",
            ]
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                exit_code = main(argv)
            self.assertEqual(exit_code, 0)
            printed = json.loads(stdout.getvalue())
            self.assertIn("g1", printed)
            self.assertIn("g2", printed)
            self.assertEqual(printed["lens"]["expected_digest"], "shared-digest")

    def test_cli_expected_arm_labels_override_accepts_dry_ledgers(self) -> None:
        # Task-2 brief's dry shakedown drives the daemon with
        # `--arm M_PRIME_DRY`/`--arm R_DRY` (never the real run's
        # `m_prime`/`r`, so a DRY ledger is never mistaken for a real one).
        # The CLI must expose the library's already-tested
        # `expected_arm_labels` kwarg (`test_dry_shakedown_labels_parse_via_
        # expected_arm_labels` in test_recompute_v2.py) so a dry invocation
        # can actually reach it end-to-end.
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(
                tmp,
                TASKS,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                ledger_label_m_prime="M_PRIME_DRY",
                ledger_label_r="R_DRY",
                digest_m_prime=("shared-digest", "shared-digest"),
                digest_r=("shared-digest", "shared-digest"),
            )
            argv = [
                "--corpus-dir", str(paths["corpus_dir"]),
                "--arm-m-prime-dir", str(paths["arm_m_prime_dir"]),
                "--arm-r-dir", str(paths["arm_r_dir"]),
                "--ledger-m-prime", str(paths["ledger_m_prime"]),
                "--ledger-r", str(paths["ledger_r"]),
                "--expected-digest", "shared-digest",
                "--expected-arm-labels", "M_PRIME_DRY", "R_DRY",
            ]
            stdout = io.StringIO()
            stderr = io.StringIO()
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                exit_code = main(argv)
            self.assertEqual(exit_code, 0, stderr.getvalue())
            printed = json.loads(stdout.getvalue())
            self.assertEqual(printed["lens"]["arm_labels"], {"m_prime": "M_PRIME_DRY", "r": "R_DRY"})

    def test_cli_expected_arm_labels_defaults_to_m_prime_r_when_omitted(self) -> None:
        # Mutation-catching partner to the override test above: DRY-labeled
        # ledgers, but the flag is OMITTED -- the CLI's default must still be
        # the real-run labels ('m_prime', 'r'), so this must FATAL (proving
        # the default isn't silently permissive / the flag isn't ignored).
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(
                tmp,
                TASKS,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                ledger_label_m_prime="M_PRIME_DRY",
                ledger_label_r="R_DRY",
                digest_m_prime=("shared-digest", "shared-digest"),
                digest_r=("shared-digest", "shared-digest"),
            )
            argv = [
                "--corpus-dir", str(paths["corpus_dir"]),
                "--arm-m-prime-dir", str(paths["arm_m_prime_dir"]),
                "--arm-r-dir", str(paths["arm_r_dir"]),
                "--ledger-m-prime", str(paths["ledger_m_prime"]),
                "--ledger-r", str(paths["ledger_r"]),
                "--expected-digest", "shared-digest",
            ]
            stdout = io.StringIO()
            stderr = io.StringIO()
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                exit_code = main(argv)
            self.assertNotEqual(exit_code, 0)
            self.assertEqual(stdout.getvalue(), "")
            self.assertIn("FATAL", stderr.getvalue())
            self.assertIn("arm-label check failed", stderr.getvalue())

    def test_cli_fatal_and_nonzero_on_incomplete_arm(self) -> None:
        # Review finding IMPORTANT-1: a truncated arm must FATAL the CLI,
        # even when digests match cleanly.
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(
                tmp,
                TASKS,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                digest_m_prime=("shared-digest", "shared-digest"),
                digest_r=("shared-digest", "shared-digest"),
                r_skip_p2={"t0"},
            )
            argv = [
                "--corpus-dir", str(paths["corpus_dir"]),
                "--arm-m-prime-dir", str(paths["arm_m_prime_dir"]),
                "--arm-r-dir", str(paths["arm_r_dir"]),
                "--ledger-m-prime", str(paths["ledger_m_prime"]),
                "--ledger-r", str(paths["ledger_r"]),
                "--expected-digest", "shared-digest",
            ]
            stdout = io.StringIO()
            stderr = io.StringIO()
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                exit_code = main(argv)
            self.assertNotEqual(exit_code, 0)
            self.assertEqual(stdout.getvalue(), "")
            self.assertIn("FATAL", stderr.getvalue())
            self.assertIn("incomplete", stderr.getvalue())
            self.assertIn("'r'", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
