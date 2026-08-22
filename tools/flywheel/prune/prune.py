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

WHAT A SAVED CHECKPOINT OWES ITS READER (all three learned the hard way on
the Task-B rental run — see `.superpowers/sdd/2026-08-22-reap-observer/
task-B-report.md` §B4/§B5/§B9):

1. **Index tensors go on the parameter's device.** A CPU index against
   CUDA weights is a hard `index_select` failure, so `--device cuda` was
   unusable as shipped. `keep_index` builds per parameter.
2. **The directory must be standalone.** `save_pretrained` writes weights
   and config, never the tokenizer. Without it
   `AutoTokenizer.from_pretrained(out_dir)` and `convert_hf_to_gguf` both
   fail on a directory that otherwise looks complete.
3. **The config must not declare weights that are absent.** The text-only
   `Qwen3_5MoeForCausalLM` drops the MTP head
   (`_keys_to_ignore_on_load_unexpected = [r"^mtp.*"]`, modeling:1900) but
   `mtp_num_hidden_layers` survives in the config. llama.cpp then sizes
   `block_count = num_hidden_layers + mtp_num_hidden_layers`, declares one
   block more than it writes, and the GGUF dies at serve time with
   `missing tensor 'blk.40.attn_norm.weight'`. `drop_absent_mtp_layers`
   zeroes such keys — but only when the weights really are gone.
"""

from __future__ import annotations

import json
import re
import shutil
from collections.abc import Sequence
from pathlib import Path

import torch
import torch.nn as nn

from . import REAP_UPSTREAM
from .blocks import find_moe_blocks
from .saliency import KEEP_RULE_REF, REAP_FORMULA_REF, _check_routable

PROVENANCE_KEY = "reap_pruning"
SIDECAR_NAME = "reap_pruning.json"

# Config keys that declare multi-token-prediction layers. `qwen3_5_moe`
# uses the first; DeepSeek/GLM-style configs use the second.
MTP_LAYER_CONFIG_KEYS = ("mtp_num_hidden_layers", "num_nextn_predict_layers")

# State-dict names that denote MTP weights, matching both the loader's
# `^mtp.*` ignore rule and the `model.mtp.*` nesting the GGUF converter
# handles.
_MTP_NAME = re.compile(r"^(model\.)?mtp(\.|_|$)")

# Tokenizer artifacts copied from the source checkpoint to keep the pruned
# directory standalone. `preprocessor_config.json` is deliberately EXCLUDED:
# the pruned checkpoint is text-only, and shipping an image-processor config
# would advertise an input path the weights cannot serve.
TOKENIZER_ARTIFACTS = (
    "tokenizer.json",
    "tokenizer_config.json",
    "tokenizer.model",
    "vocab.json",
    "vocab.txt",
    "merges.txt",
    "special_tokens_map.json",
    "added_tokens.json",
    "chat_template.jinja",
    "chat_template.json",
)


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
        # Last line of defence before any tensor is touched: a layer that
        # keeps fewer experts than top_k cannot route.
        _check_routable(len(keep), ref.num_experts, ref.top_k,
                        layer=ref.layer_index)
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


def keep_index(param: torch.Tensor, keep: Sequence[int]) -> torch.Tensor:
    """The gather index for `param`, **on `param`'s own device**.

    Building it on CPU is what broke `--device cuda` on the rental run:

        RuntimeError: Expected all tensors to be on the same device, but got
        index is on cpu, different from other tensors on cuda:0

    Per-parameter rather than per-layer, because `device_map` sharding can
    place the router and the expert tensors on different devices.
    """
    return torch.tensor(list(keep), dtype=torch.long, device=param.device)


def _slice_expert_dim(param: nn.Parameter, keep: Sequence[int]) -> nn.Parameter:
    sliced = param.data.index_select(0, keep_index(param, keep)).clone()
    return nn.Parameter(sliced, requires_grad=param.requires_grad)


def prune_model(model, keep_indices_per_layer: dict[int, list[int]]) -> dict:
    """Slice every MoE layer down to its kept experts, in place.

    Returns a report: counts before/after and the per-layer kept counts.
    """
    blocks = find_moe_blocks(model)
    kept = _validate(keep_indices_per_layer, blocks)
    before = blocks[0].num_experts

    with torch.no_grad():
        for ref in blocks:
            keep = sorted(keep_indices_per_layer[ref.layer_index])
            experts, gate = ref.experts, ref.gate
            experts.gate_up_proj = _slice_expert_dim(experts.gate_up_proj, keep)
            experts.down_proj = _slice_expert_dim(experts.down_proj, keep)
            gate.weight = _slice_expert_dim(gate.weight, keep)
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


def has_mtp_weights(model) -> bool:
    """Whether any multi-token-prediction weight is actually present.

    Mirrors the rule the loader itself uses to discard them —
    `_keys_to_ignore_on_load_unexpected = [r"^mtp.*"]` (modeling:1900) —
    plus the `model.mtp.*` nesting llama.cpp's converter also handles
    (`conversion/qwen.py`, `filter_tensors`). Anything the loader would
    have dropped is what "absent" has to mean here, or the two disagree.
    """
    return any(_MTP_NAME.match(name) for name in model.state_dict())


def drop_absent_mtp_layers(model) -> dict[str, int]:
    """Zero MTP layer-count config keys when the MTP weights are gone.

    The text-only `Qwen3_5MoeForCausalLM` discards `mtp.*` on load
    (modeling:1900), so a config inherited from the base still declaring
    `mtp_num_hidden_layers: 1` describes a head the checkpoint does not
    contain. llama.cpp believes the config
    (`block_count = num_hidden_layers + mtp_num_hidden_layers`) and writes
    one block fewer than it declares; the GGUF then fails at load with
    `missing tensor 'blk.<n>.attn_norm.weight'`.

    Returns `{key: previous_value}` for every key zeroed — empty when the
    config never declared an MTP head, and empty when the weights are
    genuinely present (in which case the declaration is true and stays).
    """
    if has_mtp_weights(model):
        return {}
    configs = [model.config]
    text_config = getattr(model.config, "text_config", None)
    if text_config is not None:
        configs.append(text_config)

    zeroed: dict[str, int] = {}
    for config in configs:
        for key in MTP_LAYER_CONFIG_KEYS:
            previous = getattr(config, key, None)
            if isinstance(previous, int) and previous > 0:
                setattr(config, key, 0)
                zeroed[key] = previous
    return zeroed


def _write_tokenizer(out: Path, tokenizer, source_dir) -> list[str]:
    """Make the pruned directory standalone. Returns the files written."""
    if tokenizer is not None:
        tokenizer.save_pretrained(out)
    if source_dir is not None:
        source = Path(source_dir)
        for name in TOKENIZER_ARTIFACTS:
            candidate = source / name
            if candidate.is_file() and not (out / name).exists():
                shutil.copy2(candidate, out / name)
    return sorted(p.name for p in out.iterdir()
                  if p.name in TOKENIZER_ARTIFACTS)


def save_pruned(model, out_dir, provenance: dict | None = None, *,
                tokenizer=None, source_dir=None) -> Path:
    """Write a loadable, **standalone** HF checkpoint plus the provenance.

    The compact provenance goes into `config.json` under `reap_pruning`
    (it survives `from_pretrained`); the full kept-index lists go into
    `reap_pruning.json` beside it.

    `tokenizer` and `source_dir` exist because `save_pretrained` writes
    only weights and config: without them the output directory cannot be
    loaded by `AutoTokenizer` or converted by `convert_hf_to_gguf`.
    """
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)

    mtp_zeroed = drop_absent_mtp_layers(model)

    sidecar = None
    if provenance is not None:
        sidecar = dict(provenance)
        sidecar["mtp_dropped"] = bool(mtp_zeroed)
        sidecar["mtp_config_keys_zeroed"] = dict(mtp_zeroed)
        compact = {k: v for k, v in sidecar.items()
                   if k != "kept_indices_per_layer"}
        setattr(model.config, PROVENANCE_KEY, compact)

    model.save_pretrained(out)
    _write_tokenizer(out, tokenizer, source_dir)
    if sidecar is not None:
        (out / SIDECAR_NAME).write_text(json.dumps(sidecar, indent=2,
                                                   sort_keys=True) + "\n")
    return out
