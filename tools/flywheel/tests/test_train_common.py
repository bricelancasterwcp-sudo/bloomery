import json, tempfile, unittest
from pathlib import Path

try:
    import torch  # noqa: F401
    from tools.flywheel import train_common as tc
    from tools.flywheel.tests.train_fixture import build_action_tokenizer, tiny_corpus, ACTION_END
    HAVE_TORCH = True
except Exception:  # stdlib python: skip cleanly like the prune tests
    HAVE_TORCH = False


@unittest.skipUnless(HAVE_TORCH, "needs ~/flywheel-venv")
class PinnedRecipe(unittest.TestCase):
    def test_constants_are_turn4_values(self):
        self.assertEqual((tc.MAX_SEQ, tc.LORA_R, tc.LORA_ALPHA, tc.PROCEDURE_SEED), (4096, 16, 32, 20260816))

    def test_training_args_are_turn4_values(self):
        a = tc.training_args(Path("/tmp/x"))
        self.assertEqual((a.num_train_epochs, a.per_device_train_batch_size, a.per_device_eval_batch_size,
                          a.gradient_accumulation_steps, a.learning_rate, a.lr_scheduler_type, a.warmup_steps,
                          a.logging_steps, a.eval_strategy, a.eval_steps, a.save_strategy, a.bf16, a.seed, a.max_steps),
                         (2, 1, 1, 8, 2e-4, "cosine", 20, 10, "steps", 100, "no", True, 20260816, -1))
        self.assertEqual(a.report_to, [])

    def test_tokenize_masks_prompt_and_ends_at_action_close(self):
        tok = build_action_tokenizer()
        row = tc.tokenize_fn(tok)(tiny_corpus(1)[0])
        n_prompt = len(tok("t1 t2 t0 ", add_special_tokens=True)["input_ids"])
        self.assertEqual(row["labels"][:n_prompt], [-100] * n_prompt)
        self.assertEqual(row["labels"][n_prompt:], row["input_ids"][n_prompt:])
        self.assertTrue(tok.decode(row["input_ids"][-4:]).rstrip().endswith(ACTION_END))
        self.assertEqual(len(row["attention_mask"]), len(row["input_ids"]))

    def test_load_pairs_filters_val_split(self):
        with tempfile.TemporaryDirectory() as d:
            corpus = Path(d) / "c.jsonl"; fp = Path(d) / "f.json"
            corpus.write_text("".join(json.dumps(r) + "\n" for r in tiny_corpus(6)))
            fp.write_text(json.dumps({"val_split_ids": ["task-1", "task-4"]}))
            train, val = tc.load_pairs(corpus, fp)
            self.assertEqual((len(train), len(val)), (4, 2))
