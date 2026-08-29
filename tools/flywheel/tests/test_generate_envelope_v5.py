"""Tests for the v5 ideal assembler and its pipeline threading (turn-7
spec §2.1–§2.2): `generate_envelope_v5`'s mechanical rules, the
`evidence_line_of` construction contract, the `--envelope` flag's
byte-identity promise, and the v5-only row-meta keys.

The per-template evidence ground truth is proven beside the templates
(the same split as the symptom-mismatch Y-present proofs); the structural
check over a WHOLE generated corpus is `check_corpus_v5.py`'s and is
tested with it. This file owns the assembler's own contracts.
"""

import json
import unittest

from tools.flywheel.factory import generate_envelope_v5 as gev5
from tools.flywheel.factory import generate_request
from tools.flywheel.factory.task import (
    DEFECT_ABSENT,
    MISSING_TARGET,
    REFUSAL_FAMILIES,
    SYMPTOM_MISMATCH,
    RefusalTask,
    Task,
    done_v5,
    evidence_line_of,
)
from tools.flywheel.tests.test_generate import REAL_TOOL, run_generate


def _patch_task(**overrides) -> Task:
    fields = {
        "name": "t_patch",
        "lens": "python",
        "target": "meter.py",
        "files": {"meter.py": "def f():\n    b = 1\n    return b\n"},
        "goal": "Fix meter.py. Patch the file, then emit done.",
        "search": "b = 1",
        "replace": "b = 2",
        "summary": "Set b to 2.",
    }
    fields.update(overrides)
    return Task(**fields)


def _refusal_task(**overrides) -> RefusalTask:
    files = {"cfg.txt": "retries: 3\nwindow: 9\ndepth: 4\n"}
    fields = {
        "name": "t_refuse",
        "lens": "plaintext",
        "family": DEFECT_ABSENT,
        "target": "cfg.txt",
        "target_missing": False,
        "files": files,
        "goal": "cfg.txt says `window: 9` is wrong. Check first, and only patch if it is genuinely wrong; then emit done.",
        "refusal_reason": "No change needed: window is 9 as documented.",
        "evidence": (evidence_line_of(files, "cfg.txt", "window: 9"),),
    }
    fields.update(overrides)
    return RefusalTask(**fields)


class FamilyReasonInversionTest(unittest.TestCase):
    def test_the_mapping_is_the_endpoint_tables_exact_inversion(self):
        self.assertEqual(
            gev5.FAMILY_TO_REASON,
            {
                "defect_absent": "no-defect",
                "missing_target": "no-such-file",
                "symptom_mismatch": "different-defect",
            },
        )

    def test_the_mapping_covers_exactly_the_factory_families(self):
        self.assertEqual(sorted(gev5.FAMILY_TO_REASON), sorted(REFUSAL_FAMILIES))


class EvidenceLineOfTest(unittest.TestCase):
    FILES = {"a.py": "alpha\nreturn total\nomega\n"}

    def test_derives_the_one_based_line(self):
        self.assertEqual(evidence_line_of(self.FILES, "a.py", "return total"), ("a.py", 2, "return total"))

    def test_empty_quote_raises(self):
        with self.assertRaises(ValueError):
            evidence_line_of(self.FILES, "a.py", "   ")

    def test_unknown_path_raises(self):
        with self.assertRaises(ValueError):
            evidence_line_of(self.FILES, "b.py", "return total")

    def test_absent_quote_raises(self):
        with self.assertRaises(ValueError):
            evidence_line_of(self.FILES, "a.py", "return nothing")

    def test_ambiguous_quote_raises(self):
        files = {"a.py": "x = 1\ny = 2\nx = 1\n"}
        with self.assertRaises(ValueError):
            evidence_line_of(files, "a.py", "x = 1")


class PatchEvidenceTest(unittest.TestCase):
    def test_single_line_replace_quotes_the_post_patch_line(self):
        task = _patch_task()
        self.assertEqual(gev5.patch_evidence(task), ("meter.py", 2, "    b = 2"))

    def test_mid_line_search_quotes_the_full_post_patch_line(self):
        task = _patch_task(
            files={"meter.py": "total = alpha + beta\n"},
            search="alpha + beta",
            replace="alpha - beta",
        )
        self.assertEqual(gev5.patch_evidence(task), ("meter.py", 1, "total = alpha - beta"))

    def test_multi_line_region_picks_the_first_differing_line(self):
        task = _patch_task(
            files={"meter.py": "def f():\n    x = 1\n    return x\n"},
            search="    x = 1\n    return x",
            replace="    x = 1\n    return x + 1",
        )
        self.assertEqual(gev5.patch_evidence(task), ("meter.py", 3, "    return x + 1"))

    def test_trailing_deletion_falls_back_to_the_regions_first_line(self):
        task = _patch_task(files={"meter.py": "a\nb\nc\n"}, search="b\nc", replace="b")
        self.assertEqual(gev5.patch_evidence(task), ("meter.py", 2, "b"))

    def test_a_blank_only_change_is_a_factory_bug(self):
        task = _patch_task(files={"meter.py": "a\nx\n"}, search="x", replace="")
        with self.assertRaises(ValueError):
            gev5.patch_evidence(task)


class ToV5TaskTest(unittest.TestCase):
    def test_patch_summary_becomes_the_exact_declared_block(self):
        task = _patch_task()
        v5 = gev5.to_v5_task(task)
        self.assertEqual(
            v5.summary,
            '<action verb="done" outcome="patched" reason="fixed">\n'
            "evidence: meter.py:2 `    b = 2`\n"
            "Set b to 2.\n"
            "</action>",
        )
        # Everything except the ideal is untouched.
        self.assertEqual(v5._replace(summary=task.summary), task)

    def test_defect_absent_reason_is_no_defect(self):
        task = _refusal_task()
        v5 = gev5.to_v5_task(task)
        self.assertEqual(
            v5.refusal_reason,
            '<action verb="done" outcome="refused" reason="no-defect">\n'
            "evidence: cfg.txt:2 `window: 9`\n"
            "No change needed: window is 9 as documented.\n"
            "</action>",
        )
        self.assertEqual(v5._replace(refusal_reason=task.refusal_reason), task)

    def test_symptom_mismatch_reason_is_different_defect(self):
        files = {"cfg.txt": "retries: 3\nretries: 5\n"}
        task = _refusal_task(
            family=SYMPTOM_MISMATCH,
            files=files,
            evidence=(evidence_line_of(files, "cfg.txt", "retries: 5"),),
        )
        v5 = gev5.to_v5_task(task)
        self.assertIn('outcome="refused" reason="different-defect"', v5.refusal_reason)
        self.assertIn("evidence: cfg.txt:2 `retries: 5`", v5.refusal_reason)

    def test_missing_target_evidence_is_the_mechanical_absent_line(self):
        task = _refusal_task(
            family=MISSING_TARGET,
            target="gone.txt",
            target_missing=True,
            files={"other.txt": "still here\n"},
            goal="gone.txt is broken. Check first, and only patch if it is genuinely wrong; then emit done.",
            refusal_reason="Cannot: gone.txt does not exist in this workspace.",
            evidence=(),
        )
        v5 = gev5.to_v5_task(task)
        self.assertIn("evidence: gone.txt absent\n", v5.refusal_reason)
        self.assertIn('reason="no-such-file"', v5.refusal_reason)

    def test_target_present_without_ground_truth_is_a_factory_bug(self):
        task = _refusal_task(evidence=())
        with self.assertRaises(ValueError):
            gev5.to_v5_task(task)

    def test_the_block_is_done_v5s_own_output_never_a_second_formatter(self):
        # Mutation guard for the single-home rule: the assembler must emit
        # byte-for-byte what done_v5 emits for the same parts.
        task = _refusal_task()
        path, line_no, quote = task.evidence[0]
        self.assertEqual(
            gev5.to_v5_task(task).refusal_reason,
            done_v5(
                outcome="refused",
                reason="no-defect",
                evidence_lines=[gev5.format_evidence_line(path, line_no, quote)],
                prose=task.refusal_reason,
            ),
        )


class V5RowMetaTest(unittest.TestCase):
    def test_patch_meta_gains_envelope_and_replace_under_v5_only(self):
        task = _patch_task()
        v4 = generate_request.row_meta("t1", task, "done")
        v5 = generate_request.row_meta("t1", task, "done", generate_request.V5_ENVELOPE)
        self.assertEqual(set(v5) - set(v4), {"envelope", "replace"})
        self.assertEqual(v5["envelope"], "v5")
        self.assertEqual(v5["replace"], task.replace)
        self.assertEqual({k: v5[k] for k in v4}, v4)

    def test_refuse_meta_gains_envelope_and_family_under_v5_only(self):
        task = _refusal_task()
        v4 = generate_request.row_meta("t1", task, "done")
        v5 = generate_request.row_meta("t1", task, "done", generate_request.V5_ENVELOPE)
        self.assertEqual(set(v5) - set(v4), {"envelope", "family"})
        self.assertEqual(v5["family"], DEFECT_ABSENT)
        self.assertEqual({k: v5[k] for k in v4}, v4)

    def test_the_wire_request_is_stamped_with_the_given_envelope(self):
        task = _patch_task()
        self.assertEqual(generate_request.build_trajectory_request(task)["envelope"], "v4")
        self.assertEqual(
            generate_request.build_trajectory_request(task, generate_request.V5_ENVELOPE)["envelope"],
            "v5",
        )


@unittest.skipUnless(REAL_TOOL is not None, "flywheel-tool binary not built; run cargo build -p bloomery-daemon --bin flywheel-tool")
class RealToolV5PipelineTest(unittest.TestCase):
    """End-to-end through the real tool: under --envelope v5 every done
    row's completion is the declared block VERBATIM (the tool parses it
    with the real bloomery-core parser and re-emits it unwrapped), and the
    v4 path is byte-identical whether the flag is omitted or explicit."""

    def _generate(self, tmp, extra):
        out, report = tmp / "out.jsonl", tmp / "report.json"
        run_generate(
            ["--seed", "31", "--count", "6", "--refusal-count", "6", "--tool", str(REAL_TOOL),
             *extra, "--out", str(out), "--report", str(report)]
        )
        return out, report

    def test_v5_done_rows_are_declared_blocks_and_meta_is_stamped(self):
        import tempfile
        from pathlib import Path

        with tempfile.TemporaryDirectory() as raw:
            out, report = self._generate(Path(raw), ["--envelope", "v5"])
            rows = [json.loads(line) for line in out.read_text(encoding="utf-8").splitlines() if line.strip()]
            done_rows = [r for r in rows if r["meta"]["pair"] == "done"]
            self.assertTrue(done_rows)
            for row in rows:
                self.assertEqual(row["meta"]["envelope"], "v5")
            for row in done_rows:
                self.assertTrue(row["completion"].startswith('<action verb="done" outcome="'))
                self.assertTrue(row["completion"].rstrip().endswith("</action>"))
                self.assertIn("\nevidence: ", "\n" + row["completion"].split("\n", 1)[1])
            fingerprint = json.loads(report.read_text(encoding="utf-8"))
            self.assertEqual(fingerprint["envelope"], "v5")

    def test_omitting_the_flag_equals_explicit_v4_byte_for_byte(self):
        import tempfile
        from pathlib import Path

        with tempfile.TemporaryDirectory() as raw_a, tempfile.TemporaryDirectory() as raw_b:
            out_a, report_a = self._generate(Path(raw_a), [])
            out_b, report_b = self._generate(Path(raw_b), ["--envelope", "v4"])
            self.assertEqual(out_a.read_bytes(), out_b.read_bytes())
            self.assertEqual(report_a.read_bytes(), report_b.read_bytes())
            self.assertNotIn("envelope", json.loads(report_a.read_text(encoding="utf-8")))


if __name__ == "__main__":
    unittest.main()
