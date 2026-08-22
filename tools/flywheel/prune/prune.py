"""Expert-dimension surgery, config rewrite and checkpoint save.

Everything the expert count touches, and what happens to it:

    experts.gate_up_proj  [E, 2I, H]  -> [K, 2I, H]   sliced on dim 0
    experts.down_proj     [E, H,  I]  -> [K, H,  I]   sliced on dim 0
    gate.weight           [E, H]      -> [K, H]       rows sliced
    experts.num_experts, gate.num_experts             set to K
    config.num_experts (+ config.text_config)         set to K
    model.num_experts (aux-loss bookkeeping, :1908)   set to K

    shared_expert, shared_expert_gate                 UNTOUCHED
    attention / Gated-DeltaNet / norms / embeddings   UNTOUCHED

`gate.weight` is the row that maps a hidden state to expert `e`'s logit
(modeling_qwen3_5_moe.py:764). Forgetting to slice it leaves a router that
scores K+ experts against K expert tensors — the checkpoint then either
fails to load (shape mismatch against the rebuilt module) or, worse,
indexes past the end of the sliced expert tensor at run time.

The keep order is the ascending index order from `select_experts_to_prune`,
so kept expert `j` of the pruned model is old expert `keep[j]`; the mapping
is recorded in the provenance sidecar.
"""

from __future__ import annotations

import json
from pathlib import Path

import torch
import torch.nn as nn

from . import REAP_UPSTREAM
from .blocks import find_moe_blocks
from .saliency import KEEP_RULE_REF, REAP_FORMULA_REF

PROVENANCE_KEY = "reap_pruning"
SIDECAR_NAME = "reap_pruning.json"


def _validate(keep_indices_per_layer: dict[int, list[int]],
              blocks) -> int:
    layers = {ref.layer_index for ref in blocks}
    given = set(keep_indices_per_layer)
    if given != layers:
        raise ValueError(
            f"keep map covers layers {sorted(given)} but the model has "
            f"{sorted(layers)}")
    sizes = set()
    for ref in blocks:
        keep = keep_indices_per_layer[ref.layer_index]
        if not keep:
            raise ValueError(f"layer {ref.layer_index}: keep list is empty")
        if len(set(keep)) != len(keep):
            raise ValueError(f"layer {ref.layer_index}: duplicate keep indices")
        bad = [i for i in keep if not 0 <= i < ref.num_experts]
        if bad:
            raise ValueError(
                f"layer {ref.layer_index}: keep indices {bad} outside "
                f"[0, {ref.num_experts})")
        sizes.add(len(keep))
    if len(sizes) != 1:
        raise ValueError(
            f"layers keep different expert counts {sorted(sizes)}; this "
            "architecture stores one `config.num_experts` for all layers")
    return sizes.pop()


def _set_num_experts(model, kept: int) -> None:
    config = model.config
    if hasattr(config, "num_experts"):
        config.num_experts = kept
    text_config = getattr(config, "text_config", None)
    if text_config is not None and hasattr(text_config, "num_experts"):
        text_config.num_experts = kept
    if hasattr(model, "num_experts"):
        model.num_experts = kept


def prune_model(model, keep_indices_per_layer: dict[int, list[int]]) -> dict:
    """Slice every MoE layer down to its kept experts, in place.

    Returns a report: counts before/after and the per-layer kept counts.
    """
    blocks = find_moe_blocks(model)
    kept = _validate(keep_indices_per_layer, blocks)
    before = blocks[0].num_experts

    with torch.no_grad():
        for ref in blocks:
            index = torch.tensor(sorted(keep_indices_per_layer[ref.layer_index]),
                                 dtype=torch.long)
            experts, gate = ref.experts, ref.gate
            experts.gate_up_proj = nn.Parameter(
                experts.gate_up_proj.data.index_select(0, index).clone(),
                requires_grad=experts.gate_up_proj.requires_grad)
            experts.down_proj = nn.Parameter(
                experts.down_proj.data.index_select(0, index).clone(),
                requires_grad=experts.down_proj.requires_grad)
            gate.weight = nn.Parameter(
                gate.weight.data.index_select(0, index).clone(),
                requires_grad=gate.weight.requires_grad)
            experts.num_experts = kept
            gate.num_experts = kept

    _set_num_experts(model, kept)
    return {
        "num_experts_before": before,
        "num_experts_after": kept,
        "num_layers": len(blocks),
        "kept_per_layer": {ref.layer_index: kept for ref in blocks},
    }


def build_provenance(*, report: dict, keep_indices_per_layer: dict,
                     compression: float, rounding: str, seed: int,
                     calibration: dict, renormalize_router_weights: bool,
                     metric: str = "reap") -> dict:
    """The record that travels with the checkpoint.

    `kept_indices_per_layer` is split out into the sidecar — at 40 layers x
    134 experts it is ~40 kB, too much to bury in `config.json`.
    """
    return {
        "tool": "tools.flywheel.prune",
        "method": "reap",
        "saliency_metric": metric,
        "reap_upstream": REAP_UPSTREAM,
        "reap_formula": REAP_FORMULA_REF,
        "keep_rule": f"{KEEP_RULE_REF} (rounding={rounding})",
        "compression": compression,
        "rounding": rounding,
        "seed": seed,
        "renormalize_router_weights": renormalize_router_weights,
        "num_experts_before": report["num_experts_before"],
        "num_experts_after": report["num_experts_after"],
        "num_layers": report["num_layers"],
        "kept_per_layer": {str(k): v
                           for k, v in sorted(report["kept_per_layer"].items())},
        "calibration": dict(calibration),
        "kept_indices_per_layer": {
            str(k): sorted(int(i) for i in v)
            for k, v in sorted(keep_indices_per_layer.items())},
    }


def save_pruned(model, out_dir, provenance: dict | None = None) -> Path:
    """Write a loadable HF checkpoint plus the pruning provenance.

    The compact provenance goes into `config.json` under `reap_pruning`
    (it survives `from_pretrained`); the full kept-index lists go into
    `reap_pruning.json` beside it.
    """
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)

    sidecar = None
    if provenance is not None:
        sidecar = dict(provenance)
        compact = {k: v for k, v in provenance.items()
                   if k != "kept_indices_per_layer"}
        setattr(model.config, PROVENANCE_KEY, compact)

    model.save_pretrained(out)
    if sidecar is not None:
        (out / SIDECAR_NAME).write_text(json.dumps(sidecar, indent=2,
                                                   sort_keys=True) + "\n")
    return out
