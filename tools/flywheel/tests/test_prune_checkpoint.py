"""Tests for what a SAVED pruned checkpoint owes its reader.

Three bugs, all found by the Task-B rental run at full scale after the unit
suite had passed — see `.superpowers/sdd/2026-08-22-reap-observer/
task-B-report.md` §B4/§B5/§B9:

  #1  the keep index was built on CPU, so `--device cuda` died in
      `index_select` (`DeviceSlicingTest`)
  #2  `save_pretrained` writes no tokenizer, so the output directory was
      not standalone (`TokenizerArtifactTest`)
  #3  the config declared an MTP block the weights no longer contained, so
      the GGUF was unloadable (`MtpConsistencyTest`; the end-to-end
      conversion guard lives in `test_prune_gguf.py`)

Split out of `test_prune_model.py` to keep both files under the 400-line
house cap — the same reasoning `test_generate_refusal.py` used.
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
from safetensors import safe_open
from transformers import AutoTokenizer

from tools.flywheel.prune.blocks import find_moe_blocks
from tools.flywheel.prune.prune import (
    MTP_LAYER_CONFIG_KEYS,
    build_provenance,
    keep_index,
    prune_model,
    save_pruned,
)
from tools.flywheel.tests.prune_fixture import (
    NUM_EXPERTS,
    build_mini_model,
    build_mini_tokenizer,
    mini_input_ids,
)

KEEP = [0, 1, 2, 4, 6]
HAS_CUDA = torch.cuda.is_available()


def _keep_map(keep=KEEP):
    return {i: list(keep) for i in range(4)}


class DeviceSlicingTest(unittest.TestCase):
    """Task-B bug #1: the keep index was always built on CPU, so
    `--device cuda` died in `index_select` with

        RuntimeError: Expected all tensors to be on the same device, but got
        index is on cpu, different from other tensors on cuda:0
    """

    def test_keep_index_lands_on_the_parameter_device_cpu(self):
        param = torch.zeros(8, 4)
        index = keep_index(param, KEEP)
        self.assertEqual(index.device, param.device)
        self.assertEqual(index.dtype, torch.long)
        self.assertEqual(index.tolist(), KEEP)

    @unittest.skipUnless(HAS_CUDA, "no CUDA device on this box")
    def test_keep_index_lands_on_the_parameter_device_cuda(self):
        param = torch.zeros(8, 4, device="cuda")
        index = keep_index(param, KEEP)
        self.assertEqual(index.device, param.device)
        self.assertEqual(index.device.type, "cuda")

    @unittest.skipUnless(HAS_CUDA, "no CUDA device on this box")
    def test_prunes_a_model_living_on_cuda(self):
        model = build_mini_model().to("cuda")
        report = prune_model(model, _keep_map())
        self.assertEqual(report["num_experts_after"], len(KEEP))
        for ref in find_moe_blocks(model):
            self.assertEqual(ref.experts.gate_up_proj.shape[0], len(KEEP))
            self.assertEqual(ref.gate.weight.shape[0], len(KEEP))
            # Still on the GPU: slicing must not silently migrate weights.
            self.assertEqual(ref.experts.gate_up_proj.device.type, "cuda")
            self.assertEqual(ref.gate.weight.device.type, "cuda")
        with torch.no_grad():
            logits = model(input_ids=mini_input_ids().to("cuda")).logits
        self.assertTrue(torch.isfinite(logits).all())


class TokenizerArtifactTest(unittest.TestCase):
    """Task-B bug #2: the pruned directory was not self-contained —
    `AutoTokenizer.from_pretrained(out_dir)` failed and
    `convert_hf_to_gguf` had no vocab to read.
    """

    def _pruned(self):
        model = build_mini_model()
        prune_model(model, _keep_map())
        return model

    def test_saved_directory_loads_with_auto_tokenizer(self):
        with TemporaryDirectory() as tmp:
            out = Path(tmp) / "pruned"
            save_pruned(self._pruned(), out, tokenizer=build_mini_tokenizer())
            self.assertTrue((out / "tokenizer.json").exists())
            tokenizer = AutoTokenizer.from_pretrained(out)
            self.assertEqual(tokenizer("t1 t2")["input_ids"], [1, 2])

    def test_extra_tokenizer_artifacts_are_copied_from_the_source(self):
        with TemporaryDirectory() as tmp:
            source = Path(tmp) / "base"
            source.mkdir()
            (source / "chat_template.jinja").write_text("{{ messages }}")
            (source / "merges.txt").write_text("#version: 0.2\n")
            out = Path(tmp) / "pruned"
            save_pruned(self._pruned(), out, tokenizer=build_mini_tokenizer(),
                        source_dir=source)
            self.assertEqual((out / "chat_template.jinja").read_text(),
                             "{{ messages }}")
            self.assertTrue((out / "merges.txt").exists())

    def test_vision_preprocessor_config_is_not_carried_over(self):
        # The pruned checkpoint is text-only; advertising an image processor
        # it cannot honour is the same class of lie as bug #3.
        with TemporaryDirectory() as tmp:
            source = Path(tmp) / "base"
            source.mkdir()
            (source / "preprocessor_config.json").write_text("{}")
            (source / "tokenizer_config.json").write_text("{}")
            out = Path(tmp) / "pruned"
            save_pruned(self._pruned(), out, source_dir=source)
            self.assertFalse((out / "preprocessor_config.json").exists())

    def test_save_without_a_tokenizer_still_works(self):
        with TemporaryDirectory() as tmp:
            out = Path(tmp) / "pruned"
            save_pruned(self._pruned(), out)
            self.assertTrue((out / "config.json").exists())


class MtpConsistencyTest(unittest.TestCase):
    """Task-B bug #3: the pruned config kept `mtp_num_hidden_layers: 1`
    while the text-only load had dropped the MTP weights, so
    `convert_hf_to_gguf` declared 41 blocks and wrote 40 — the GGUF failed
    at serve time with `missing tensor 'blk.40.attn_norm.weight'`.

    `_Qwen35MtpMixin.__init__` (llama.cpp conversion/qwen.py) does
    `block_count = num_hidden_layers + hparams.get("mtp_num_hidden_layers", 0)`,
    which is the exact mechanism.
    """

    def _pruned_with_mtp(self, value=1):
        model = build_mini_model()
        model.config.mtp_num_hidden_layers = value
        prune_model(model, _keep_map())
        return model

    def test_mtp_layer_count_is_zeroed_when_no_mtp_weights_exist(self):
        model = self._pruned_with_mtp()
        self.assertEqual([n for n in model.state_dict() if "mtp" in n], [])
        with TemporaryDirectory() as tmp:
            out = Path(tmp) / "pruned"
            save_pruned(model, out)
            config = json.loads((out / "config.json").read_text())
            self.assertEqual(config["mtp_num_hidden_layers"], 0)

    def test_declared_blocks_match_the_layers_actually_written(self):
        model = self._pruned_with_mtp()
        with TemporaryDirectory() as tmp:
            out = Path(tmp) / "pruned"
            save_pruned(model, out)
            config = json.loads((out / "config.json").read_text())
            declared = (config["num_hidden_layers"]
                        + config.get("mtp_num_hidden_layers", 0))
            written = set()
            for shard in out.glob("*.safetensors"):
                with safe_open(shard, framework="pt") as handle:
                    for name in handle.keys():
                        if ".layers." in name:
                            written.add(name.split(".layers.")[1].split(".")[0])
            self.assertEqual(declared, len(written))

    def test_no_mtp_tensor_reaches_the_saved_shards(self):
        model = self._pruned_with_mtp()
        with TemporaryDirectory() as tmp:
            out = Path(tmp) / "pruned"
            save_pruned(model, out)
            for shard in out.glob("*.safetensors"):
                with safe_open(shard, framework="pt") as handle:
                    self.assertEqual(
                        [n for n in handle.keys() if "mtp" in n], [])

    def test_provenance_records_the_dropped_mtp_head(self):
        model = self._pruned_with_mtp()
        report = {"num_experts_before": NUM_EXPERTS,
                  "num_experts_after": len(KEEP), "num_layers": 4,
                  "kept_per_layer": {i: len(KEEP) for i in range(4)}}
        prov = build_provenance(
            report=report, keep_indices_per_layer=_keep_map(),
            compression=0.48, rounding="floor", seed=42,
            calibration={"source": "unit-test"},
            renormalize_router_weights=False)
        with TemporaryDirectory() as tmp:
            out = Path(tmp) / "pruned"
            save_pruned(model, out, provenance=prov)
            config = json.loads((out / "config.json").read_text())
            self.assertIs(config["reap_pruning"]["mtp_dropped"], True)
            self.assertEqual(
                config["reap_pruning"]["mtp_config_keys_zeroed"],
                {"mtp_num_hidden_layers": 1})
            sidecar = json.loads((out / "reap_pruning.json").read_text())
            self.assertIs(sidecar["mtp_dropped"], True)

    def test_mtp_is_not_flagged_when_the_config_never_declared_one(self):
        model = build_mini_model()
        prune_model(model, _keep_map())
        prov = build_provenance(
            report={"num_experts_before": NUM_EXPERTS,
                    "num_experts_after": len(KEEP), "num_layers": 4,
                    "kept_per_layer": {i: len(KEEP) for i in range(4)}},
            keep_indices_per_layer=_keep_map(), compression=0.48,
            rounding="floor", seed=42, calibration={},
            renormalize_router_weights=False)
        with TemporaryDirectory() as tmp:
            out = Path(tmp) / "pruned"
            save_pruned(model, out, provenance=prov)
            config = json.loads((out / "config.json").read_text())
            self.assertIs(config["reap_pruning"]["mtp_dropped"], False)
            self.assertEqual(config["reap_pruning"]["mtp_config_keys_zeroed"],
                             {})

    def test_mtp_config_key_survives_when_mtp_weights_are_present(self):
        # Only a head that is genuinely absent may be zeroed. A model that
        # still carries `mtp.*` weights keeps its declaration.
        model = self._pruned_with_mtp()
        # A realistic MTP head: state-dict keys `mtp.weight` / `mtp.bias`.
        model.add_module("mtp", torch.nn.Linear(4, 4))
        self.assertTrue(any(n.startswith("mtp.") for n in model.state_dict()))
        with TemporaryDirectory() as tmp:
            out = Path(tmp) / "pruned"
            save_pruned(model, out)
            config = json.loads((out / "config.json").read_text())
            self.assertEqual(config["mtp_num_hidden_layers"], 1)

    def test_all_known_mtp_keys_are_handled(self):
        self.assertIn("mtp_num_hidden_layers", MTP_LAYER_CONFIG_KEYS)
        self.assertIn("num_nextn_predict_layers", MTP_LAYER_CONFIG_KEYS)


if __name__ == "__main__":
    unittest.main()
