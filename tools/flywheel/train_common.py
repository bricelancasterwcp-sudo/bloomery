"""Binding rules shared by `train.py` (turns 1-4, unsloth QLoRA) and
`train_moe.py` (turn 5, bf16 LoRA on qwen3_5_moe). Moved here 2026-08-22
without behaviour change; pinned by `tests/test_train_common.py`."""

import json
from pathlib import Path

import torch

MAX_SEQ = 4096
LORA_R = 16
LORA_ALPHA = 32
PROCEDURE_SEED = 20260816


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


PINNED_ARGS = dict(num_train_epochs=2, per_device_train_batch_size=1, per_device_eval_batch_size=1,
                   gradient_accumulation_steps=8, learning_rate=2e-4, lr_scheduler_type="cosine",
                   warmup_steps=20, logging_steps=10, eval_strategy="steps", eval_steps=100,
                   save_strategy="no", bf16=True, report_to=[], seed=PROCEDURE_SEED)


def training_args(out: Path, max_steps: int = -1, **overrides):
    """Turn 1-4's TrainingArguments, verbatim. `overrides` exist ONLY for
    CPU smoke tests (bf16=False, use_cpu=True); a pre-registered run passes none."""
    from transformers import TrainingArguments
    kwargs = dict(PINNED_ARGS, output_dir=str(out), max_steps=max_steps)
    kwargs.update(overrides)
    return TrainingArguments(**kwargs)
