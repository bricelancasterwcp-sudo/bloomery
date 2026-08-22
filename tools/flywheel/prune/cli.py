"""`python -m tools.flywheel.prune.cli` — calibrate, select, prune, save.

Example (the shape the rental run will use):

    ~/flywheel-venv/bin/python -m tools.flywheel.prune.cli \
        --model ~/models/hf/Qwen3.6-35B-A3B \
        --calib ~/flywheel4/corpus.jsonl \
        --samples 512 --seq-len 4096 \
        --compression 0.48 --seed 42 \
        --out ~/models/hf/Qwen3.6-35B-A3B-reap48 \
        --device cuda --dtype bf16

The model is always loaded with `Qwen3_5MoeForCausalLM` — the text-only
class. If the checkpoint is the multimodal `Qwen3_5MoeForConditionalGeneration`
wrapper, this drops the vision tower, which is what the spike did (§S5) and
what a text-only prune wants. The vision tower is NOT carried into the
output checkpoint; say so when handing the artefact on.

Only the JSON summary goes to stdout; progress goes to stderr.
"""

from __future__ import annotations

import argparse
import json
import resource
import sys
import time
from pathlib import Path

import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

from . import PruneConfigurationError
from .calib import load_calibration_texts, run_calibration, select_samples
from .prune import build_provenance, prune_model, save_pruned
from .saliency import (METRICS, ROUNDINGS, keep_count, metrics_help,
                       saliency_matrix, saliency_quantiles,
                       select_experts_to_prune)

DTYPES = {"bf16": torch.bfloat16, "fp16": torch.float16, "fp32": torch.float32}


def _log(message: str) -> None:
    print(message, file=sys.stderr, flush=True)


def _peak_rss_mb() -> float:
    # ru_maxrss is kilobytes on Linux.
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024.0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="python -m tools.flywheel.prune.cli",
        description="REAP expert pruning for qwen3_5_moe checkpoints")
    parser.add_argument("--model", required=True, type=Path,
                        help="local HF checkpoint directory")
    parser.add_argument("--calib", required=True, type=Path,
                        help="calibration corpus (.jsonl or plain text)")
    parser.add_argument("--out", required=True, type=Path,
                        help="output directory for the pruned checkpoint")
    parser.add_argument("--tokenizer", type=Path, default=None,
                        help="tokenizer directory (defaults to --model)")
    parser.add_argument("--samples", type=int, default=None,
                        help="calibration samples to use (default: all)")
    parser.add_argument("--seq-len", type=int, default=4096)
    parser.add_argument("--compression", type=float, default=0.48,
                        help="fraction of experts to prune, in [0, 1)")
    parser.add_argument("--rounding", choices=ROUNDINGS, default="floor",
                        help="keep-count rounding; floor matches upstream "
                             "REAP, ceil matches crucible-labs' 133/256")
    parser.add_argument("--metric", choices=METRICS, default="reap",
                        help=metrics_help())
    parser.add_argument("--renormalize-router-weights", action="store_true",
                        help="weight by the renormalised top-k gate value "
                             "(what the block actually applies) instead of "
                             "the raw softmax probability")
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--device", default="cpu")
    parser.add_argument("--dtype", choices=sorted(DTYPES), default="bf16")
    parser.add_argument("--save-observer", action="store_true",
                        help="also write observer_state.json next to the "
                             "pruned checkpoint")
    return parser


def _load_model(args):
    """Load the checkpoint text-only, streaming straight onto the device.

    `AutoModelForCausalLM` is the resolution the spike verified on the real
    checkpoint (§S5): on `Qwen3.6-35B-A3B`, whose `architectures` says
    `Qwen3_5MoeForConditionalGeneration`, it resolves to the text-only
    `Qwen3_5MoeForCausalLM` and drops the vision tower. `device_map` avoids
    materialising 65 GiB in host RAM before the move to GPU.
    """
    kwargs = {"dtype": DTYPES[args.dtype]}
    if args.device != "cpu":
        kwargs["device_map"] = args.device
    model = AutoModelForCausalLM.from_pretrained(args.model, **kwargs)
    if not model.__class__.__name__.startswith("Qwen3_5Moe"):
        raise SystemExit(
            f"{args.model} resolved to {model.__class__.__name__}; this tool "
            "only prunes qwen3_5_moe checkpoints")
    if args.device == "cpu":
        model = model.to("cpu")
    return model.eval()


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    started = time.time()
    torch.manual_seed(args.seed)

    if not args.model.is_dir():
        raise SystemExit(f"--model is not a directory: {args.model}")

    _log(f"loading {args.model} ({args.dtype} on {args.device})")
    model = _load_model(args)
    _log(f"loaded {model.__class__.__name__}, "
         f"{model.config.num_experts} experts / "
         f"{model.config.num_experts_per_tok} per token")

    # Decide the keep count BEFORE spending a calibration pass, so an
    # impossible compression costs nothing and writes nothing.
    planned_keep = keep_count(
        model.config.num_experts, args.compression, rounding=args.rounding,
        num_experts_per_tok=model.config.num_experts_per_tok)
    _log(f"plan: keep {planned_keep} of {model.config.num_experts} experts "
         f"per layer (compression {args.compression}, {args.rounding})")

    tokenizer = AutoTokenizer.from_pretrained(args.tokenizer or args.model)

    texts = select_samples(load_calibration_texts(args.calib), args.samples,
                           args.seed)
    _log(f"calibrating on {len(texts)} samples at seq_len {args.seq_len}")
    observer, calib_stats = run_calibration(
        model, tokenizer, texts, seq_len=args.seq_len, device=args.device,
        renormalize_router_weights=args.renormalize_router_weights,
        progress=lambda done, total: _log(f"  sample {done}/{total}")
        if done % 50 == 0 else None)
    calib_stats["source"] = str(args.calib)
    calib_stats["requested_samples"] = args.samples

    saliency = saliency_matrix(observer.state, metric=args.metric)
    keep = select_experts_to_prune(
        saliency, args.compression, rounding=args.rounding,
        num_experts_per_tok=model.config.num_experts_per_tok)
    _log(f"keeping {len(next(iter(keep.values())))} experts per layer "
         f"across {len(keep)} layers")

    report = prune_model(model, keep)
    provenance = build_provenance(
        report=report, keep_indices_per_layer=keep,
        compression=args.compression, rounding=args.rounding, seed=args.seed,
        calibration=calib_stats,
        renormalize_router_weights=args.renormalize_router_weights,
        metric=args.metric)
    # `source_dir` as well as the tokenizer object: `save_pretrained` does
    # not round-trip every artifact a base checkpoint carries, and a pruned
    # directory that cannot be tokenized is not a usable checkpoint.
    save_pruned(model, args.out, provenance=provenance, tokenizer=tokenizer,
                source_dir=args.tokenizer or args.model)
    if args.save_observer:
        (args.out / "observer_state.json").write_text(
            json.dumps(observer.to_dict(), indent=2) + "\n")

    all_scores = torch.cat([s.to(torch.float64) for s in saliency.values()])
    summary = {
        "model": str(args.model),
        "out": str(args.out),
        "metric": args.metric,
        "compression": args.compression,
        "rounding": args.rounding,
        "seed": args.seed,
        "num_experts_before": report["num_experts_before"],
        "num_experts_after": report["num_experts_after"],
        "num_layers": report["num_layers"],
        "kept_per_layer": {str(k): v
                           for k, v in sorted(report["kept_per_layer"].items())},
        "calibration": calib_stats,
        "saliency_quantiles": saliency_quantiles(all_scores),
        "wall_seconds": round(time.time() - started, 3),
        "peak_rss_mb": round(_peak_rss_mb(), 1),
    }
    if args.device.startswith("cuda") and torch.cuda.is_available():
        summary["peak_cuda_gib"] = round(
            torch.cuda.max_memory_allocated() / 1024 ** 3, 3)
    (args.out / "summary.json").write_text(json.dumps(summary, indent=2,
                                                      sort_keys=True) + "\n")
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


def run(argv: list[str] | None = None) -> int:
    """`main` with the refusals turned into a named message and exit 2."""
    try:
        return main(argv)
    except PruneConfigurationError as exc:
        _log(f"PruneConfigurationError: {exc}")
        return 2


if __name__ == "__main__":
    raise SystemExit(run())
