"""Tests for tools.flywheel.prune.prune — tensor surgery and checkpoint save.

What must survive pruning untouched: the shared expert, the shared-expert
gate, attention, and the Gated-DeltaNet token mixer. What must shrink:
`experts.gate_up_proj`, `experts.down_proj`, `gate.weight`, and every
config field that encodes the expert count.
"""

import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

try:  # pragma: no cover - environment guard
    import torch  # noqa: F401
    import transformers  # noqa: F401
except ImportError as exc:  # pragma: no cover
    raise unittest.SkipTest(f"torch/transformers unavailable: {exc}")

import torch
import torch.nn.functional as F
from transformers import Qwen3_5MoeForCausalLM

from tools.flywheel.prune.blocks import find_moe_blocks
from tools.flywheel.prune.prune import build_provenance, prune_model, save_pruned
from tools.flywheel.tests.prune_fixture import (
    NUM_EXPERTS,
    build_mini_model,
    mini_input_ids,
)

KEEP = [0, 1, 2, 4, 6]


def _keep_map(keep=KEEP):
    return {i: list(keep) for i in range(4)}


class PruneShapeTest(unittest.TestCase):
    def setUp(self):
        self.model = build_mini_model()
        self.before = {
            ref.layer_index: (
                ref.experts.gate_up_proj.detach().clone(),
                ref.experts.down_proj.detach().clone(),
                ref.gate.weight.detach().clone(),
            )
            for ref in find_moe_blocks(self.model)
        }
        self.shared_before = {
            ref.layer_index: ref.block.shared_expert.down_proj.weight.detach().clone()
            for ref in find_moe_blocks(self.model)
        }
        self.report = prune_model(self.model, _keep_map())

    def test_expert_tensors_are_sliced_on_the_expert_dimension(self):
        for ref in find_moe_blocks(self.model):
            self.assertEqual(ref.experts.gate_up_proj.shape[0], len(KEEP))
            self.assertEqual(ref.experts.down_proj.shape[0], len(KEEP))
            self.assertEqual(ref.experts.gate_up_proj.shape[1:],
                             self.before[ref.layer_index][0].shape[1:])

    def test_router_weight_rows_are_sliced(self):
        for ref in find_moe_blocks(self.model):
            want = self.before[ref.layer_index][2][torch.tensor(KEEP)]
            self.assertTrue(torch.equal(ref.gate.weight.detach(), want))

    def test_kept_expert_weights_are_bit_identical(self):
        for ref in find_moe_blocks(self.model):
            gu, dp, _ = self.before[ref.layer_index]
            self.assertTrue(torch.equal(ref.experts.gate_up_proj.detach(),
                                        gu[torch.tensor(KEEP)]))
            self.assertTrue(torch.equal(ref.experts.down_proj.detach(),
                                        dp[torch.tensor(KEEP)]))

    def test_kept_expert_output_is_unchanged_when_routed(self):
        ref = find_moe_blocks(self.model)[0]
        gu_before, dp_before, _ = self.before[0]
        x = torch.randn(3, self.model.config.hidden_size)
        original_index, new_index = KEEP[3], 3

        def run(gate_up, down, idx):
            g, u = F.linear(x, gate_up[idx]).chunk(2, dim=-1)
            return F.linear(ref.experts.act_fn(g) * u, down[idx])

        self.assertTrue(torch.equal(
            run(gu_before, dp_before, original_index),
            run(ref.experts.gate_up_proj.detach(),
                ref.experts.down_proj.detach(), new_index)))

    def test_shared_expert_is_untouched(self):
        for ref in find_moe_blocks(self.model):
            self.assertTrue(torch.equal(
                ref.block.shared_expert.down_proj.weight.detach(),
                self.shared_before[ref.layer_index]))
            self.assertEqual(ref.block.shared_expert_gate.weight.shape,
                             (1, self.model.config.hidden_size))

    def test_expert_counts_are_updated_everywhere(self):
        self.assertEqual(self.model.config.num_experts, len(KEEP))
        self.assertEqual(self.model.num_experts, len(KEEP))
        for ref in find_moe_blocks(self.model):
            self.assertEqual(ref.experts.num_experts, len(KEEP))
            self.assertEqual(ref.gate.num_experts, len(KEEP))

    def test_report_records_per_layer_counts(self):
        self.assertEqual(self.report["num_experts_before"], NUM_EXPERTS)
        self.assertEqual(self.report["num_experts_after"], len(KEEP))
        self.assertEqual(self.report["kept_per_layer"],
                         {i: len(KEEP) for i in range(4)})

    def test_pruned_model_still_runs_a_forward_pass(self):
        with torch.no_grad():
            out = self.model(input_ids=mini_input_ids())
        self.assertEqual(out.logits.shape, (1, 12, self.model.config.vocab_size))
        self.assertTrue(torch.isfinite(out.logits).all())


class PruneValidationTest(unittest.TestCase):
    def test_rejects_out_of_range_indices(self):
        model = build_mini_model()
        with self.assertRaises(ValueError):
            prune_model(model, {0: [0, 99], 1: [0], 2: [0], 3: [0]})

    def test_rejects_duplicate_indices(self):
        model = build_mini_model()
        with self.assertRaises(ValueError):
            prune_model(model, {i: [0, 0, 1] for i in range(4)})

    def test_rejects_empty_keep_list(self):
        model = build_mini_model()
        with self.assertRaises(ValueError):
            prune_model(model, {i: [] for i in range(4)})

    def test_rejects_missing_layer(self):
        model = build_mini_model()
        with self.assertRaises(ValueError):
            prune_model(model, {0: [0, 1]})

    def test_rejects_ragged_keep_counts(self):
        # A single `config.num_experts` cannot describe two different counts.
        model = build_mini_model()
        with self.assertRaises(ValueError):
            prune_model(model, {0: [0, 1], 1: [0, 1, 2], 2: [0, 1], 3: [0, 1]})


class SavePrunedTest(unittest.TestCase):
    def test_checkpoint_reloads_and_reproduces_the_in_memory_logits(self):
        model = build_mini_model()
        prune_model(model, _keep_map())
        ids = mini_input_ids()
        with torch.no_grad():
            expected = model(input_ids=ids).logits.clone()

        with TemporaryDirectory() as tmp:
            out = Path(tmp) / "pruned"
            save_pruned(model, out, provenance={"kept_per_layer": {0: 5}})
            self.assertTrue((out / "config.json").exists())
            self.assertTrue(any(out.glob("*.safetensors")))

            reloaded = Qwen3_5MoeForCausalLM.from_pretrained(out).eval()
            reloaded.set_experts_implementation("eager")
            self.assertEqual(reloaded.config.num_experts, len(KEEP))
            for ref in find_moe_blocks(reloaded):
                self.assertEqual(ref.experts.gate_up_proj.shape[0], len(KEEP))
                self.assertEqual(ref.gate.weight.shape[0], len(KEEP))
            with torch.no_grad():
                got = reloaded(input_ids=ids).logits
            self.assertTrue(torch.equal(expected, got),
                            f"max delta {(expected - got).abs().max().item()}")

    def test_provenance_lands_in_config_and_sidecar(self):
        model = build_mini_model()
        report = prune_model(model, _keep_map())
        prov = build_provenance(
            report=report,
            keep_indices_per_layer=_keep_map(),
            compression=0.48,
            rounding="floor",
            seed=42,
            calibration={"source": "unit-test", "samples": 3, "seq_len": 8,
                         "tokens": 24},
            renormalize_router_weights=False,
        )
        with TemporaryDirectory() as tmp:
            out = Path(tmp) / "pruned"
            save_pruned(model, out, provenance=prov)
            cfg = json.loads((out / "config.json").read_text())
            recorded = cfg["reap_pruning"]
            self.assertEqual(recorded["num_experts_before"], NUM_EXPERTS)
            self.assertEqual(recorded["num_experts_after"], len(KEEP))
            self.assertEqual(recorded["compression"], 0.48)
            self.assertEqual(recorded["seed"], 42)
            self.assertEqual(recorded["calibration"]["samples"], 3)
            self.assertEqual(recorded["kept_per_layer"], {"0": 5, "1": 5,
                                                          "2": 5, "3": 5})
            self.assertNotIn("kept_indices_per_layer", recorded)

            sidecar = json.loads((out / "reap_pruning.json").read_text())
            self.assertEqual(sidecar["kept_indices_per_layer"]["0"], KEEP)

            reloaded = Qwen3_5MoeForCausalLM.from_pretrained(out)
            self.assertEqual(reloaded.config.reap_pruning["seed"], 42)


if __name__ == "__main__":
    unittest.main()
