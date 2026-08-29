"""Tests for `tools.flywheel.check_corpus_v5` (turn-7 spec §2.4), all
mutation-guarded: one clean two-task corpus must PASS, and for every rule
one minimal corrupted variant of it must FAIL with a violation naming
that rule. The clean rows are built with the factory's own assemblers
(`done_v5`, `format_evidence_line`) — the checker validates OUR
generator's authored output, so the clean fixture and the generator must
share one shape or the pass is meaningless; every corruption is a
hand-made drift from those bytes.

The wrong-line-number variant is the one that proves the checker keeps
the endpoint's `misaligned` verdict apart from `ungrounded` (a true
quote on the wrong line vs a fabrication), and the pre-patch-only-quote
variant is the one that proves patch rows are classified against
POST-patch bytes, never `target_contents` as stored."""

import copy
import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

from tools.flywheel import check_corpus_v5
from tools.flywheel.factory.generate_envelope_v5 import format_evidence_line
from tools.flywheel.factory.task import done_v5

APP_CONTENTS = "def add(a, b):\n    return a - b\n"
NOTES_CONTENTS = "alpha\nbravo\ncharlie\n"
PATCH_PROSE = "Replaced the subtraction in add() with addition."
REFUSE_PROSE = "Checked: no defect in notes.txt — bravo is correct. No change made."


def _patch_task_rows():
    """One plain-shaped patch task's three pair rows (read/patch/done)."""
    files = {"app.py": APP_CONTENTS}
    base = {
        "task_id": "0001",
        "template": "swap-sign",
        "lens": "python",
        "expect": "patch",
        "trajectory": "plain",
        "goal": "add() subtracts its operands; make it add them",
        "target": "app.py",
        "target_contents": APP_CONTENTS,
        "files": files,
        "search": "    return a - b",
        "replace": "    return a + b",
        "envelope": "v5",
    }
    completions = {
        "read": '<action verb="read" path="app.py">\n</action>',
        "patch": (
            '<action verb="patch" path="app.py">\n<<<<<<< SEARCH\n    return a - b\n'
            "=======\n    return a + b\n>>>>>>> REPLACE\n</action>"
        ),
        "done": done_v5(
            outcome="patched",
            reason="fixed",
            evidence_lines=[format_evidence_line("app.py", 2, "    return a + b")],
            prose=PATCH_PROSE,
        ),
    }
    return [
        {"prompt": f"P{i}", "completion": completions[pair], "meta": {**base, "pair": pair}}
        for i, pair in enumerate(("read", "patch", "done"), 1)
    ]


def _refuse_task_rows():
    """One defect-absent refuse task's two pair rows (read/done)."""
    files = {"notes.txt": NOTES_CONTENTS}
    base = {
        "task_id": "0002",
        "template": "phantom-typo",
        "lens": "plaintext",
        "expect": "refuse",
        "family": "defect_absent",
        "goal": "fix the typo on line 2 of notes.txt",
        "target": "notes.txt",
        "target_contents": NOTES_CONTENTS,
        "files": files,
        "search": "",
        "envelope": "v5",
    }
    completions = {
        "read": '<action verb="read" path="notes.txt">\n</action>',
        "done": done_v5(
            outcome="refused",
            reason="no-defect",
            evidence_lines=[format_evidence_line("notes.txt", 2, "bravo")],
            prose=REFUSE_PROSE,
        ),
    }
    return [
        {"prompt": f"P{i}", "completion": completions[pair], "meta": {**base, "pair": pair}}
        for i, pair in enumerate(("read", "done"), 1)
    ]


def _clean_rows():
    return copy.deepcopy(_patch_task_rows() + _refuse_task_rows())


def _row(rows, task_id, pair):
    (match,) = [r for r in rows if r["meta"]["task_id"] == task_id and r["meta"]["pair"] == pair]
    return match


def _run(rows):
    """Writes `rows` to a temp jsonl, runs the checker's real CLI entry
    (with --json), returns (exit code, summary dict, stdout text)."""
    with tempfile.TemporaryDirectory() as tmpdir:
        corpus = Path(tmpdir) / "corpus.jsonl"
        corpus.write_text(
            "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows), encoding="utf-8"
        )
        out = Path(tmpdir) / "summary.json"
        stdout = io.StringIO()
        with redirect_stdout(stdout):
            code = check_corpus_v5.main(["--corpus", str(corpus), "--json", str(out)])
        summary = json.loads(out.read_text(encoding="utf-8"))
    return code, summary, stdout.getvalue()


class CheckCorpusV5Test(unittest.TestCase):
    def _assert_fails(self, rows, *fragments):
        code, summary, _ = _run(rows)
        self.assertEqual(code, 2)
        self.assertGreaterEqual(summary["violations"], 1)
        reported = "\n".join(summary["violations_reported"])
        for fragment in fragments:
            self.assertIn(fragment, reported)
        return summary

    def test_clean_two_task_corpus_passes_with_the_registered_counts(self):
        code, summary, stdout = _run(_clean_rows())
        self.assertEqual(code, 0)
        self.assertEqual(summary["violations"], 0)
        self.assertEqual(summary["violations_reported"], [])
        self.assertEqual(summary["rows"], 5)
        self.assertEqual(summary["tasks"], 2)
        self.assertEqual(summary["done_rows"], 2)
        self.assertEqual(summary["tasks_by_expect"], {"patch": 1, "refuse": 1})
        self.assertEqual(summary["refuse_tasks_by_family"], {"defect_absent": 1})
        self.assertEqual(summary["patch_tasks_by_trajectory"], {"plain": 1})
        # stdout carries the same summary --json wrote (one report, two sinks).
        self.assertEqual(json.loads(stdout), summary)

    def test_fabricated_quote_fails_ungrounded(self):
        rows = _clean_rows()
        _row(rows, "0002", "done")["completion"] = done_v5(
            outcome="refused",
            reason="no-defect",
            evidence_lines=[format_evidence_line("notes.txt", 2, "delta")],
            prose=REFUSE_PROSE,
        )
        self._assert_fails(rows, "rule 5", "'ungrounded'")

    def test_true_quote_on_wrong_line_fails_misaligned_not_ungrounded(self):
        rows = _clean_rows()
        _row(rows, "0002", "done")["completion"] = done_v5(
            outcome="refused",
            reason="no-defect",
            evidence_lines=[format_evidence_line("notes.txt", 1, "bravo")],
            prose=REFUSE_PROSE,
        )
        summary = self._assert_fails(rows, "rule 5", "'misaligned'")
        self.assertNotIn("ungrounded", "\n".join(summary["violations_reported"]))

    def test_quote_existing_only_pre_patch_fails_against_post_patch_bytes(self):
        rows = _clean_rows()
        _row(rows, "0001", "done")["completion"] = done_v5(
            outcome="patched",
            reason="fixed",
            evidence_lines=[format_evidence_line("app.py", 2, "    return a - b")],
            prose=PATCH_PROSE,
        )
        self._assert_fails(rows, "rule 5", "'ungrounded'")

    def test_missing_replace_on_a_patch_row_is_a_violation(self):
        rows = _clean_rows()
        del _row(rows, "0001", "done")["meta"]["replace"]
        self._assert_fails(rows, "rule 5", "meta.replace")

    def test_reason_swapped_against_family_fails(self):
        rows = _clean_rows()
        _row(rows, "0002", "done")["completion"] = done_v5(
            outcome="refused",
            reason="no-such-file",
            evidence_lines=[format_evidence_line("notes.txt", 2, "bravo")],
            prose=REFUSE_PROSE,
        )
        self._assert_fails(rows, "rule 4", "'no-defect'")

    def test_bare_undeclared_done_fails(self):
        rows = _clean_rows()
        _row(rows, "0002", "done")["completion"] = (
            f'<action verb="done">\n{REFUSE_PROSE}\n</action>'
        )
        self._assert_fails(rows, "rule 3")

    def test_empty_prose_fails(self):
        rows = _clean_rows()
        _row(rows, "0002", "done")["completion"] = (
            '<action verb="done" outcome="refused" reason="no-defect">\n'
            "evidence: notes.txt:2 `bravo`\n"
            "</action>"
        )
        self._assert_fails(rows, "rule 3", "prose")

    def test_refuse_row_missing_family_fails(self):
        rows = _clean_rows()
        del _row(rows, "0002", "done")["meta"]["family"]
        self._assert_fails(rows, "rule 4", "family")

    def test_envelope_v4_on_any_row_fails(self):
        rows = _clean_rows()
        _row(rows, "0002", "read")["meta"]["envelope"] = "v4"
        self._assert_fails(rows, "rule 1", "'v4'")

    def test_non_done_pair_carrying_a_done_block_fails(self):
        rows = _clean_rows()
        declared = _row(rows, "0001", "done")["completion"]
        _row(rows, "0001", "read")["completion"] = declared
        self._assert_fails(rows, "rule 2")


if __name__ == "__main__":
    unittest.main()
