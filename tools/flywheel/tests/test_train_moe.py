import json, tempfile, unittest
from pathlib import Path

try:
    import torch
    from tools.flywheel import train_moe
    from tools.flywheel.tests.prune_fixture import build_mini_model
    from tools.flywheel.tests.train_fixture import build_action_tokenizer, tiny_corpus
    HAVE_TORCH = True
except Exception:
    HAVE_TORCH = False

EXPECTED_SUFFIXES = {"q_proj", "k_proj", "v_proj", "o_proj", "in_proj_qkv", "in_proj_z", "in_proj_b",
                     "in_proj_a", "out_proj", "gate_proj", "up_proj", "down_proj"}


@unittest.skipUnless(HAVE_TORCH, "needs ~/flywheel-venv")
class LoraTargets(unittest.TestCase):
    def test_targets_hit_attention_deltanet_and_shared_expert_only(self):
        m = train_moe.apply_lora(build_mini_model())
        wrapped = [n for n, mod in m.named_modules() if hasattr(mod, "lora_A")]
        self.assertTrue(wrapped)
        self.assertEqual({n.rsplit(".", 1)[-1] for n in wrapped}, EXPECTED_SUFFIXES)
        self.assertFalse([n for n in wrapped if ".experts." in n or n.endswith("mlp.gate")])
        self.assertTrue(all("shared_expert" in n for n in wrapped if n.endswith(("gate_proj", "up_proj", "down_proj"))))

    def test_experts_and_router_are_frozen(self):
        m = train_moe.apply_lora(build_mini_model())
        stats = train_moe.assert_frozen(m)
        for n, p in m.named_parameters():
            if ".experts." in n or n.endswith("mlp.gate.weight"):
                self.assertFalse(p.requires_grad, n)
        self.assertGreater(stats["trainable"], 0)
        self.assertLess(stats["trainable"] / stats["total"], 0.2)  # mini model; real: 0.0611%

    def test_same_seed_same_init(self):
        a = train_moe.apply_lora(build_mini_model(), seed=20260816)
        b = train_moe.apply_lora(build_mini_model(), seed=20260816)
        sa = {n: p for n, p in a.named_parameters() if "lora_" in n}
        for n, p in b.named_parameters():
            if "lora_" in n:
                self.assertTrue(torch.equal(p, sa[n]), n)


@unittest.skipUnless(HAVE_TORCH, "needs ~/flywheel-venv")
class CpuSmoke(unittest.TestCase):
    def test_two_steps_on_cpu_write_the_markers(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            base = d / "base"; build_mini_model().save_pretrained(base); build_action_tokenizer().save_pretrained(base)
            corpus = d / "c.jsonl"; fp = d / "f.json"
            corpus.write_text("".join(json.dumps(r) + "\n" for r in tiny_corpus(6)))
            fp.write_text(json.dumps({"val_split_ids": ["task-5"]}))
            out = d / "adapter"
            rc = train_moe.main(["--corpus", str(corpus), "--fingerprint", str(fp), "--base", str(base),
                                 "--out", str(out), "--max-steps", "2", "--device", "cpu", "--dtype", "float32"])
            self.assertEqual(rc, 0)
            self.assertTrue((out / "DONE").exists())
            self.assertEqual((out / "EXIT").read_text().strip(), "0")
            self.assertTrue((out / "adapter_config.json").exists())
            self.assertTrue((out / "tokenizer_config.json").exists())
