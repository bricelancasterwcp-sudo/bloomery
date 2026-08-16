"""Flywheel turn-1 QLoRA training — the pre-registered run.

Governing documents:
- spec  docs/superpowers/specs/2026-08-16-flywheel-14b-design.md §4
- gates docs/superpowers/evidence/2026-08-16-flywheel1-preregistration.md

Binding rules implemented here (do not change without a recorded
amendment):
- RAW text, no chat template: sample = prompt + completion exactly as the
  corpus carries them (the corpus prompts were rendered by the serving
  code via flywheel-tool).
- Completion-only loss: prompt tokens are label-masked (-100).
- NO EOS appended: each sample's final token decodes to the tail of
  "</action>" — asserted on a real batch before training starts.
- Validation split (fingerprint val_split_ids) is filtered OUT of the
  train set and used for loss monitoring only.

Usage:
  python tools/flywheel/train.py --corpus ~/flywheel1/corpus.jsonl \
      --fingerprint ~/flywheel1/fingerprint.json \
      --base ~/models/hf/Qwen3-14B --out ~/flywheel1/adapter \
      [--max-steps N]   # smoke: --max-steps 5
"""

import argparse
import json
from pathlib import Path

import torch
from unsloth import FastLanguageModel
from transformers import Trainer, TrainingArguments

MAX_SEQ = 4096
LORA_R = 16
LORA_ALPHA = 32
TARGET_MODULES = ["q_proj", "k_proj", "v_proj", "o_proj",
                  "gate_proj", "up_proj", "down_proj"]


def load_pairs(corpus_path: Path, fingerprint_path: Path):
    val_ids = set(json.loads(fingerprint_path.read_text())["val_split_ids"])
    train, val = [], []
    with corpus_path.open() as f:
        for line in f:
            row = json.loads(line)
            bucket = val if row["meta"]["task_id"] in val_ids else train
            bucket.append({"prompt": row["prompt"],
                           "completion": row["completion"]})
    return train, val


class PairDataset(torch.utils.data.Dataset):
    """Plain torch dataset over pre-tokenized rows (no hf-datasets)."""

    def __init__(self, rows):
        self.rows = rows

    def __len__(self):
        return len(self.rows)

    def __getitem__(self, i):
        return self.rows[i]


def collate_single(batch):
    """bs=1 collator: tensorize the one sample, no padding needed."""
    assert len(batch) == 1, "train AND eval batch size must stay 1"
    row = batch[0]
    return {k: torch.tensor([v]) for k, v in row.items()}


def tokenize_fn(tokenizer):
    def fn(sample):
        # BOS on the prompt; nothing appended after the completion — the
        # last token must be the tail of "</action>" (asserted below).
        prompt_ids = tokenizer(sample["prompt"],
                               add_special_tokens=True)["input_ids"]
        completion_ids = tokenizer(sample["completion"],
                                   add_special_tokens=False)["input_ids"]
        input_ids = (prompt_ids + completion_ids)[:MAX_SEQ]
        n_prompt = min(len(prompt_ids), MAX_SEQ)
        labels = [-100] * n_prompt + input_ids[n_prompt:]
        return {"input_ids": input_ids,
                "attention_mask": [1] * len(input_ids),
                "labels": labels}
    return fn


def assert_batch_shape(tokenizer, ds):
    """The pre-registered label check: prompt masked, tail is '</action>'."""
    row = ds[0]
    n_masked = sum(1 for x in row["labels"] if x == -100)
    assert 0 < n_masked < len(row["labels"]), "prompt mask missing or total"
    tail = tokenizer.decode(row["input_ids"][-4:])
    assert tail.rstrip().endswith("</action>"), (
        f"sample must end at </action> with no EOS; tail={tail!r}")
    print(f"label-check ok: {n_masked} prompt tokens masked, tail={tail!r}")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", required=True, type=Path)
    ap.add_argument("--fingerprint", required=True, type=Path)
    ap.add_argument("--base", required=True)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--max-steps", type=int, default=-1)
    args = ap.parse_args()

    model, tokenizer = FastLanguageModel.from_pretrained(
        args.base, max_seq_length=MAX_SEQ, load_in_4bit=True)
    model = FastLanguageModel.get_peft_model(
        model, r=LORA_R, lora_alpha=LORA_ALPHA,
        target_modules=TARGET_MODULES, lora_dropout=0.0, bias="none",
        use_gradient_checkpointing="unsloth", random_state=20260816)

    train_rows, val_rows = load_pairs(args.corpus, args.fingerprint)
    print(f"pairs: train={len(train_rows)} val={len(val_rows)}")
    # No hf-datasets at all: every Dataset construction fingerprints via
    # dill, which is broken on Python 3.14 (Pickler._batch_setitems
    # signature change). We tokenize eagerly and feed a plain torch
    # Dataset to transformers.Trainer; at bs=1 the collator just
    # tensorizes the single sample.
    fn = tokenize_fn(tokenizer)
    train_ds = PairDataset([fn(r) for r in train_rows])
    val_ds = PairDataset([fn(r) for r in val_rows])
    assert_batch_shape(tokenizer, train_ds)

    targs = TrainingArguments(
        output_dir=str(args.out), num_train_epochs=2,
        max_steps=args.max_steps,
        per_device_train_batch_size=1, per_device_eval_batch_size=1,
        gradient_accumulation_steps=8,
        learning_rate=2e-4, lr_scheduler_type="cosine", warmup_steps=20,
        logging_steps=10, eval_strategy="steps", eval_steps=100,
        save_strategy="no", bf16=True, report_to=[], seed=20260816)
    trainer = Trainer(model=model, args=targs, data_collator=collate_single,
                      train_dataset=train_ds, eval_dataset=val_ds)
    trainer.train()
    model.save_pretrained(str(args.out))
    tokenizer.save_pretrained(str(args.out))
    (args.out / "DONE").write_text("ok\n")
    print(f"adapter saved to {args.out}")


if __name__ == "__main__":
    main()
