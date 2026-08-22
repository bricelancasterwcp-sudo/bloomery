"""End-to-end guard for Task-B bug #3: the pruned checkpoint must convert
to a GGUF whose declared `block_count` matches the blocks actually written.

On the pod the pruned `config.json` kept `mtp_num_hidden_layers: 1` while
the text-only `Qwen3_5MoeForCausalLM` load had dropped the MTP weights.
`_Qwen35MtpMixin.__init__` (llama.cpp `conversion/qwen.py`) does

    block_count = num_hidden_layers + hparams.get("mtp_num_hidden_layers", 0)

so it declared 41 blocks and emitted `qwen35moe.nextn_predict_layers = 1`
while writing only 40 — and llama.cpp refused the file at load with
`missing tensor 'blk.40.attn_norm.weight'`. Nothing before serve time
noticed.

This reproduces the whole loop at miniature scale (5 declared vs 4 written
before the fix) by running the real converter and reading the result back
with `gguf.GGUFReader`.

Two environment caveats, both handled by skipping rather than by pretending:

1. **Which converter.** The copy pinned under the `llama-cpp-sys-2` crate
   ships `convert_hf_to_gguf.py` without its `conversion/` package or
   `gguf-py`, so it cannot run (`ModuleNotFoundError: No module named
   'conversion'`). A full checkout is needed. Set `LLAMA_CPP_DIR` to point
   at one; `~/llama.cpp` is tried by default.
2. **The synthetic tokenizer.** `get_vocab_base_pre` identifies a known
   pre-tokenizer by hashing it, and the miniature word-level tokenizer is
   not in its table (`NotImplementedError: BPE pre-tokenizer was not
   recognized`). The driver overrides that one method on the concrete
   model class. That is a harness workaround for a fixture limitation —
   it touches vocab identification only and cannot affect `block_count`,
   which is computed from the config in `__init__` before any vocab work.

The converter runs in a subprocess so that llama.cpp's vendored `gguf-py`
never shadows the venv's `gguf` inside the test process.
"""

import json
import os
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

from tools.flywheel.prune.prune import prune_model, save_pruned
from tools.flywheel.tests.prune_fixture import (
    build_mini_model,
    build_mini_tokenizer,
)

# Exit 3 = the converter is not usable here (skip). Exit 4 = the converter
# ran and REFUSED our checkpoint (fail). Conflating the two would let a real
# regression — e.g. dropping the tokenizer files again — pass as a skip.
SKIP_EXIT = 3
CONVERSION_FAILED_EXIT = 4

DRIVER = '''
import json, pathlib, sys
llama_dir, model_dir, out_path = sys.argv[1:4]
sys.path.insert(0, llama_dir)
sys.path.insert(1, str(pathlib.Path(llama_dir) / "gguf-py"))
try:
    import gguf
    from conversion import (ModelBase, ModelType, get_model_architecture,
                            get_model_class)
except Exception as exc:
    print(f"converter unavailable: {type(exc).__name__}: {exc}",
          file=sys.stderr)
    raise SystemExit(3)

model_dir = pathlib.Path(model_dir)
try:
    arch = get_model_architecture(ModelBase.load_hparams(model_dir, False),
                                  ModelType.TEXT)
    cls = get_model_class(arch, False)
except Exception as exc:
    print(f"architecture unsupported: {type(exc).__name__}: {exc}",
          file=sys.stderr)
    raise SystemExit(3)

# Fixture workaround, not a behaviour change: the miniature word-level
# tokenizer is not in the converter's pre-tokenizer hash table. Vocab
# identification cannot influence block_count.
cls.get_vocab_base_pre = lambda self, tokenizer: "gpt-2"

try:
    model = cls(dir_model=model_dir, ftype=gguf.LlamaFileType.ALL_F32,
                fname_out=pathlib.Path(out_path), is_big_endian=False,
                model_name=None)
    model.write()
except Exception as exc:
    # The converter works but rejected THIS checkpoint. That is a finding,
    # not a missing dependency — exit 4 so the test fails, not skips.
    print(f"conversion failed: {type(exc).__name__}: {exc}", file=sys.stderr)
    raise SystemExit(4)

reader = gguf.GGUFReader(out_path)
fields = {name: field.contents() for name, field in reader.fields.items()}
blocks = sorted({int(t.name.split(".")[1]) for t in reader.tensors
                 if t.name.startswith("blk.")})
declared = next((v for k, v in fields.items() if k.endswith(".block_count")),
                None)
nextn = next((v for k, v in fields.items()
              if k.endswith(".nextn_predict_layers")), None)
print(json.dumps({"declared_block_count": declared,
                  "blocks_written": len(blocks),
                  "block_indices": blocks,
                  "nextn_predict_layers": nextn}))
'''


def _llama_cpp_dir():
    candidates = [os.environ.get("LLAMA_CPP_DIR"), str(Path.home() / "llama.cpp")]
    for candidate in candidates:
        if candidate and (Path(candidate) / "convert_hf_to_gguf.py").is_file():
            return Path(candidate)
    return None


class GgufBlockCountTest(unittest.TestCase):
    def setUp(self):
        self.llama_dir = _llama_cpp_dir()
        if self.llama_dir is None:
            self.skipTest("no runnable llama.cpp checkout "
                          "(set LLAMA_CPP_DIR); the llama-cpp-sys-2 pin "
                          "ships convert_hf_to_gguf.py without conversion/")

    def _convert(self, model_dir, tmp):
        driver = Path(tmp) / "driver.py"
        driver.write_text(DRIVER)
        out = Path(tmp) / "mini.gguf"
        proc = subprocess.run(
            [sys.executable, str(driver), str(self.llama_dir),
             str(model_dir), str(out)],
            capture_output=True, text=True)
        if proc.returncode == SKIP_EXIT:
            self.skipTest(f"converter unusable here: "
                          f"{proc.stderr.strip().splitlines()[-1:]}")
        if proc.returncode == CONVERSION_FAILED_EXIT:
            self.fail("convert_hf_to_gguf refused the pruned checkpoint — "
                      "the saved directory is not convertible:\n"
                      + proc.stderr[-3000:])
        self.assertEqual(proc.returncode, 0, proc.stderr[-3000:])
        return json.loads(proc.stdout.splitlines()[-1])

    def _save_pruned_mini(self, out, mtp_value):
        model = build_mini_model()
        if mtp_value is not None:
            model.config.mtp_num_hidden_layers = mtp_value
        prune_model(model, {i: [0, 1, 2, 4, 6] for i in range(4)})
        save_pruned(model, out, tokenizer=build_mini_tokenizer())

    def test_declared_block_count_matches_the_blocks_written(self):
        with TemporaryDirectory() as tmp:
            model_dir = Path(tmp) / "pruned"
            self._save_pruned_mini(model_dir, mtp_value=1)
            result = self._convert(model_dir, tmp)
            self.assertEqual(result["declared_block_count"],
                             result["blocks_written"],
                             f"GGUF declares {result['declared_block_count']} "
                             f"blocks but wrote {result['blocks_written']}: "
                             f"{result['block_indices']}")
            self.assertEqual(result["blocks_written"], 4)
            self.assertIn(result["nextn_predict_layers"], (None, 0))

    def test_the_unfixed_config_would_have_produced_the_pod_failure(self):
        """The bug, reproduced: hand the converter the config as it came
        off the pod (mtp declared, no MTP weights) and it over-declares."""
        with TemporaryDirectory() as tmp:
            model_dir = Path(tmp) / "unfixed"
            self._save_pruned_mini(model_dir, mtp_value=1)
            config_path = model_dir / "config.json"
            config = json.loads(config_path.read_text())
            config["mtp_num_hidden_layers"] = 1  # put the bug back
            config_path.write_text(json.dumps(config, indent=2))

            result = self._convert(model_dir, tmp)
            self.assertEqual(result["blocks_written"], 4)
            self.assertEqual(result["declared_block_count"], 5)
            self.assertEqual(result["nextn_predict_layers"], 1)


if __name__ == "__main__":
    unittest.main()
