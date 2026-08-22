"""Tests for tools.flywheel.prune.calib and .cli — calibration loading and
the end-to-end command line, exercised on the miniature model.
"""

import json
import subprocess
import sys
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

try:  # pragma: no cover - environment guard
    import torch  # noqa: F401
    import transformers  # noqa: F401
except ImportError as exc:  # pragma: no cover
    raise unittest.SkipTest(f"torch/transformers unavailable: {exc}")

from tools.flywheel.prune.calib import load_calibration_texts
from tools.flywheel.tests.prune_fixture import (
    build_mini_dense_model,
    build_mini_model,
    build_mini_tokenizer,
    mini_calibration_texts,
)

REPO_ROOT = Path(__file__).resolve().parents[3]


def _run_cli(argv: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, "-m", "tools.flywheel.prune.cli", *argv],
        cwd=REPO_ROOT, capture_output=True, text=True)


class CalibrationLoaderTest(unittest.TestCase):
    def test_jsonl_prompt_completion_rows_are_concatenated(self):
        with TemporaryDirectory() as tmp:
            path = Path(tmp) / "corpus.jsonl"
            path.write_text(
                json.dumps({"prompt": "ask ", "completion": "answer"}) + "\n"
                + json.dumps({"prompt": "solo"}) + "\n")
            self.assertEqual(load_calibration_texts(path),
                             ["ask answer", "solo"])

    def test_jsonl_text_rows_and_bare_strings(self):
        with TemporaryDirectory() as tmp:
            path = Path(tmp) / "c.jsonl"
            path.write_text(json.dumps({"text": "alpha"}) + "\n"
                            + json.dumps("beta") + "\n")
            self.assertEqual(load_calibration_texts(path), ["alpha", "beta"])

    def test_blank_lines_and_empty_rows_are_dropped(self):
        with TemporaryDirectory() as tmp:
            path = Path(tmp) / "c.jsonl"
            path.write_text('{"text": "keep"}\n\n{"text": "   "}\n')
            self.assertEqual(load_calibration_texts(path), ["keep"])

    def test_plain_text_file_splits_on_blank_lines(self):
        with TemporaryDirectory() as tmp:
            path = Path(tmp) / "c.txt"
            path.write_text("first para\nstill first\n\nsecond para\n")
            self.assertEqual(load_calibration_texts(path),
                             ["first para\nstill first", "second para"])

    def test_unparseable_jsonl_row_raises_with_the_line_number(self):
        with TemporaryDirectory() as tmp:
            path = Path(tmp) / "c.jsonl"
            path.write_text('{"text": "ok"}\n{not json}\n')
            with self.assertRaises(ValueError) as ctx:
                load_calibration_texts(path)
            self.assertIn("line 2", str(ctx.exception))

    def test_missing_file_raises(self):
        with self.assertRaises(FileNotFoundError):
            load_calibration_texts(Path("/nonexistent/corpus.jsonl"))


class CliSmokeTest(unittest.TestCase):
    def test_end_to_end_prunes_saves_and_reports(self):
        from transformers import Qwen3_5MoeForCausalLM

        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "mini"
            build_mini_model().save_pretrained(src)
            build_mini_tokenizer().save_pretrained(src)

            calib = root / "calib.jsonl"
            calib.write_text("".join(
                json.dumps({"text": t}) + "\n"
                for t in mini_calibration_texts(n=5)))

            out = root / "pruned"
            proc = _run_cli(
                ["--model", str(src), "--calib", str(calib),
                 "--samples", "4", "--seq-len", "8",
                 "--compression", "0.48", "--seed", "42",
                 "--out", str(out), "--device", "cpu", "--dtype", "fp32"])
            self.assertEqual(proc.returncode, 0, proc.stderr[-4000:])

            summary = json.loads(proc.stdout)
            self.assertEqual(summary["num_experts_before"], 8)
            self.assertEqual(summary["num_experts_after"], 5)
            self.assertEqual(summary["compression"], 0.48)
            self.assertEqual(summary["seed"], 42)
            self.assertEqual(summary["calibration"]["samples"], 4)
            self.assertEqual(summary["kept_per_layer"],
                             {"0": 5, "1": 5, "2": 5, "3": 5})
            self.assertEqual(sorted(summary["saliency_quantiles"]),
                             ["max", "mean", "min", "p10", "p50", "p90"])
            self.assertGreater(summary["wall_seconds"], 0.0)
            self.assertIn("peak_rss_mb", summary)

            self.assertEqual(json.loads((out / "summary.json").read_text()),
                             summary)
            # Task-B bug #2: the pruned dir must be standalone.
            from transformers import AutoTokenizer
            self.assertTrue((out / "tokenizer.json").exists())
            self.assertEqual(
                AutoTokenizer.from_pretrained(out)("t1 t2")["input_ids"],
                [1, 2])
            reloaded = Qwen3_5MoeForCausalLM.from_pretrained(out)
            self.assertEqual(reloaded.config.num_experts, 5)
            self.assertEqual(
                reloaded.model.layers[0].mlp.experts.gate_up_proj.shape[0], 5)
            self.assertEqual(reloaded.config.reap_pruning["seed"], 42)

    def test_rejects_a_missing_model_directory(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            calib = root / "c.jsonl"
            calib.write_text('{"text": "t1 t2"}\n')
            proc = _run_cli(["--model", str(root / "missing"),
                             "--calib", str(calib), "--out", str(root / "o")])
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("--model is not a directory", proc.stderr)

    def test_rejects_a_non_moe_checkpoint(self):
        """Reaches the architecture check, not the missing-directory check."""
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            dense = root / "dense"
            build_mini_dense_model().save_pretrained(dense)
            build_mini_tokenizer().save_pretrained(dense)
            calib = root / "c.jsonl"
            calib.write_text('{"text": "t1 t2"}\n')

            proc = _run_cli(["--model", str(dense), "--calib", str(calib),
                             "--out", str(root / "o"), "--device", "cpu",
                             "--dtype", "fp32"])
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("Qwen3ForCausalLM", proc.stderr)
            self.assertIn("only prunes qwen3_5_moe checkpoints", proc.stderr)
            self.assertFalse((root / "o").exists())

    def test_refuses_a_compression_that_would_break_routing(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "mini"
            build_mini_model().save_pretrained(src)
            build_mini_tokenizer().save_pretrained(src)
            calib = root / "calib.jsonl"
            calib.write_text("".join(json.dumps({"text": t}) + "\n"
                                     for t in mini_calibration_texts(n=3)))

            out = root / "pruned"
            proc = _run_cli(["--model", str(src), "--calib", str(calib),
                             "--seq-len", "8", "--compression", "0.98",
                             "--out", str(out), "--device", "cpu",
                             "--dtype", "fp32"])
            self.assertEqual(proc.returncode, 2, proc.stderr[-2000:])
            self.assertIn("PruneConfigurationError", proc.stderr)
            self.assertIn("num_experts_per_tok=2", proc.stderr)
            # Nothing written, and it never reached calibration.
            self.assertFalse(out.exists())
            self.assertNotIn("calibrating on", proc.stderr)


if __name__ == "__main__":
    unittest.main()
