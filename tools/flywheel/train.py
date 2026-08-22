"""Flywheel QLoRA training — the pre-registered run. Turn 4 (flywheel4)
uses this file UNCHANGED apart from this header; every hyperparameter
below is the turn-1 recipe, held fixed across turns 1, 2, 3 and 4.

Governing documents:
- spec  docs/superpowers/specs/2026-08-21-flywheel4-turn4-design.md §5
        (turn 3: …/2026-08-20-flywheel3-turn3-design.md §5;
         turn 1: …/2026-08-16-flywheel-14b-design.md §4)
- gates docs/superpowers/evidence/2026-08-21-flywheel4-preregistration.md
        (turn 1: …/2026-08-16-flywheel1-preregistration.md;
         turn 2: …/2026-08-16-flywheel2-preregistration.md;
         turn 3: …/2026-08-20-flywheel3-preregistration.md)

**Seeds: the two literal 20260816 seeds below (`random_state` on the LoRA
init, `seed` on TrainingArguments) do NOT move per turn and were not
changed for turn 4.** They are the *procedure's* identity — holding them
fixed is what makes turn 4 a comparison against turns 1-3 rather than a
fresh draw. The seed that refreshes each turn is the CORPUS seed
(20260817 → 20260820 → 20260821), which is recorded in the fingerprint,
not here.

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

Usage (turn 4):
  python tools/flywheel/train.py --corpus ~/flywheel4/corpus.jsonl \
      --fingerprint docs/superpowers/evidence/2026-08-21-flywheel4-fingerprint.json \
      --base ~/models/hf/Qwen3-14B --out ~/flywheel4/adapter \
      [--max-steps N]   # smoke: --max-steps 5

**Turn 5 (2026-08-22):** the shared helpers moved to `train_common.py`;
behaviour pinned by `tests/test_train_common.py`; no hyperparameter, seed,
or code path of the recipe changed.
"""

import argparse
from pathlib import Path

from unsloth import FastLanguageModel
from transformers import Trainer

from tools.flywheel.train_common import (MAX_SEQ, LORA_R, LORA_ALPHA, PROCEDURE_SEED,
                                         load_pairs, PairDataset, collate_single,
                                         tokenize_fn, assert_batch_shape, training_args)

TARGET_MODULES = ["q_proj", "k_proj", "v_proj", "o_proj",
                  "gate_proj", "up_proj", "down_proj"]


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
        use_gradient_checkpointing="unsloth", random_state=PROCEDURE_SEED)

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

    targs = training_args(args.out, args.max_steps)
    trainer = Trainer(model=model, args=targs, data_collator=collate_single,
                      train_dataset=train_ds, eval_dataset=val_ds)
    trainer.train()
    model.save_pretrained(str(args.out))
    tokenizer.save_pretrained(str(args.out))
    (args.out / "DONE").write_text("ok\n")
    print(f"adapter saved to {args.out}")


if __name__ == "__main__":
    main()
