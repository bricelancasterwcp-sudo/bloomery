"""Flywheel turn 5 — bf16 LoRA on the REAP-48-pruned Qwen3.6-35B-A3B hybrid MoE
(`Qwen3_5MoeForCausalLM`, text-only), the pre-registered rental recipe.

Governing documents:
- spec  docs/superpowers/specs/2026-08-22-flywheel5-turn5-design.md §4
- gates docs/superpowers/evidence/<date>-flywheel5-preregistration.md

What is inherited from turns 1-4 (train_common): raw text, no chat template,
completion-only loss, NO EOS (tail `</action>`), val split from the
fingerprint, TrainingArguments verbatim, the procedure seed 20260816.
What is forced by the architecture: bf16 LoRA via peft (unsloth does not
support qwen3_5_moe; bitsandbytes cannot quantize the fused 3-D expert
tensors); LoRA on attention + Gated-DeltaNet projections + the SHARED expert
only — routed experts are fused `nn.Parameter`s and the router a bare
parameter, so peft cannot wrap them; both are FROZEN and asserted so.
Unpacked, bs 1 (ruled 2026-08-22: naive packing leaks across the 30
recurrent layers' state).

Usage (turn 5, on the pod):
  python -m tools.flywheel.train_moe --corpus /workspace/flywheel5/corpus.jsonl \
      --fingerprint /workspace/flywheel5/fingerprint.json \
      --base /workspace/Qwen3.6-35B-A3B-REAP48-ours --out /workspace/flywheel5/adapter \
      [--max-steps N] [--device cuda|cpu] [--dtype bfloat16|float32]
"""
from __future__ import annotations

import argparse
import sys
import traceback
from pathlib import Path

import torch
from peft import LoraConfig, get_peft_model
from transformers import AutoModelForCausalLM, AutoTokenizer, Trainer

from tools.flywheel.train_common import (LORA_ALPHA, LORA_R, PROCEDURE_SEED, PairDataset,
                                         assert_batch_shape, collate_single, load_pairs,
                                         tokenize_fn, training_args)

TARGET_MODULES = ["q_proj", "k_proj", "v_proj", "o_proj",
                  "in_proj_qkv", "in_proj_z", "in_proj_b", "in_proj_a", "out_proj",
                  "gate_proj", "up_proj", "down_proj"]
EXPECTED_CLASS = "Qwen3_5MoeForCausalLM"


def apply_lora(model, seed: int = PROCEDURE_SEED):
    """LoRA r16/a32 on TARGET_MODULES; `torch.manual_seed(seed)` immediately
    before peft initialises the adapters (the analogue of unsloth's random_state)."""
    torch.manual_seed(seed)
    cfg = LoraConfig(r=LORA_R, lora_alpha=LORA_ALPHA, lora_dropout=0.0, bias="none",
                     target_modules=TARGET_MODULES, task_type="CAUSAL_LM")
    return get_peft_model(model, cfg)


def assert_frozen(model) -> dict:
    """Every routed-expert and router parameter must be frozen; returns counts."""
    trainable = total = 0
    for name, p in model.named_parameters():
        total += p.numel()
        if p.requires_grad:
            trainable += p.numel()
        if (".experts." in name or name.endswith("mlp.gate.weight")) and p.requires_grad:
            raise AssertionError(f"expert/router parameter is trainable: {name}")
    return {"trainable": trainable, "total": total}


def _write_markers(out: Path, rc: int) -> None:
    """The pod's wrapper reads these instead of parsing training logs —
    every `main()` exit path (refusal, unexpected exception, success)
    writes them, never just the happy path."""
    (out / "EXIT").write_text(f"{rc}\n")
    (out / "DONE").write_text("ok\n" if rc == 0 else "failed\n")


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", required=True, type=Path)
    ap.add_argument("--fingerprint", required=True, type=Path)
    ap.add_argument("--base", required=True)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--max-steps", type=int, default=-1)
    ap.add_argument("--device", default="cuda", choices=["cuda", "cpu"])
    ap.add_argument("--dtype", default="bfloat16", choices=["bfloat16", "float32"])
    args = ap.parse_args(argv)
    args.out.mkdir(parents=True, exist_ok=True)

    try:
        dtype = torch.bfloat16 if args.dtype == "bfloat16" else torch.float32
        model = AutoModelForCausalLM.from_pretrained(args.base, dtype=dtype, device_map=args.device)
        if type(model).__name__ != EXPECTED_CLASS:
            print(f"refusing: loaded {type(model).__name__}, expected {EXPECTED_CLASS}", file=sys.stderr)
            _write_markers(args.out, 2)
            return 2
        print(f"model class {EXPECTED_CLASS}; num_experts={model.config.num_experts}; "
              f"layers={model.config.num_hidden_layers}")
        tokenizer = AutoTokenizer.from_pretrained(args.base)
        model.gradient_checkpointing_enable()
        model.enable_input_require_grads()
        model = apply_lora(model)
        stats = assert_frozen(model)
        print(f"trainable {stats['trainable']} / total {stats['total']} "
              f"({100.0 * stats['trainable'] / stats['total']:.4f}%) — experts+router frozen: asserted")

        train_rows, val_rows = load_pairs(args.corpus, args.fingerprint)
        print(f"pairs: train={len(train_rows)} val={len(val_rows)}")
        fn = tokenize_fn(tokenizer)
        train_ds = PairDataset([fn(r) for r in train_rows])
        val_ds = PairDataset([fn(r) for r in val_rows])
        assert_batch_shape(tokenizer, train_ds)

        overrides = {} if args.device == "cuda" and args.dtype == "bfloat16" else {"bf16": False, "use_cpu": args.device == "cpu"}
        targs = training_args(args.out, args.max_steps, **overrides)
        trainer = Trainer(model=model, args=targs, data_collator=collate_single,
                          train_dataset=train_ds, eval_dataset=val_ds)
        trainer.train()
        model.save_pretrained(str(args.out))
        tokenizer.save_pretrained(str(args.out))
        trainer.state.save_to_json(str(args.out / "trainer_state.json"))
    except Exception as e:  # the markers are how the pod's wrapper reads the outcome
        print(f"TRAINING FAILED: {e!r}", file=sys.stderr)
        print(traceback.format_exc(), file=sys.stderr)
        _write_markers(args.out, 1)
        print("no adapter saved")
        return 1

    _write_markers(args.out, 0)
    print(f"adapter saved to {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
