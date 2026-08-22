"""Locating the `qwen3_5_moe` sparse-MoE blocks, and pinning the kernel.

Blocks are found by **class name**, never by module path. Upstream REAP
walks `model.model.layers[i]` (src/reap/model_util.py::get_moe), which
raises `AttributeError` on `Qwen3_5MoeForConditionalGeneration` because
the text stack is nested at `model.model.language_model.layers`. Matching
on `Qwen3_5MoeSparseMoeBlock` finds the blocks under either wrapper — and
under the miniature test model — with no per-architecture path table.
"""

from __future__ import annotations

import re
from contextlib import contextmanager
from dataclasses import dataclass

import torch.nn as nn

MOE_BLOCK_CLASS_NAMES = frozenset({"Qwen3_5MoeSparseMoeBlock"})

_LAYER_INDEX = re.compile(r"layers\.(\d+)\.")


@dataclass(frozen=True)
class MoEBlockRef:
    """One sparse-MoE block plus the layer index it belongs to."""

    layer_index: int
    module_name: str
    block: nn.Module

    @property
    def gate(self) -> nn.Module:
        """The router (`Qwen3_5MoeTopKRouter`). Named `gate`, not `router`."""
        return self.block.gate

    @property
    def experts(self) -> nn.Module:
        """`Qwen3_5MoeExperts` — fused 3-D `nn.Parameter` expert weights."""
        return self.block.experts

    @property
    def num_experts(self) -> int:
        return int(self.block.gate.num_experts)

    @property
    def top_k(self) -> int:
        return int(self.block.gate.top_k)


def find_moe_blocks(model: nn.Module) -> list[MoEBlockRef]:
    """Every sparse-MoE block in `model`, ordered by layer index.

    Raises if none are found: a silent empty result would turn a wrong
    model into a no-op prune that still writes a checkpoint.
    """
    refs: list[MoEBlockRef] = []
    for position, (name, module) in enumerate(model.named_modules()):
        if module.__class__.__name__ not in MOE_BLOCK_CLASS_NAMES:
            continue
        match = _LAYER_INDEX.search(name)
        index = int(match.group(1)) if match else position
        refs.append(MoEBlockRef(layer_index=index, module_name=name,
                                block=module))
    if not refs:
        raise ValueError(
            f"no MoE blocks found in {model.__class__.__name__}; expected a "
            f"module of class {sorted(MOE_BLOCK_CLASS_NAMES)}")
    indices = [ref.layer_index for ref in refs]
    if len(set(indices)) != len(indices):
        raise ValueError(f"duplicate MoE layer indices: {indices}")
    return sorted(refs, key=lambda ref: ref.layer_index)


@contextmanager
def eager_experts(model: nn.Module):
    """Force `experts_implementation="eager"` for the duration of a block.

    Why calibration must run eager: the observer reproduces the per-expert
    contribution by replaying the eager kernel body
    (`Qwen3_5MoeExperts.forward`, modeling_qwen3_5_moe.py:745-747) on the
    routed tokens. The default on this box is `grouped_mm`, a batched
    kernel whose numerics differ and whose fused output is already
    gate-weighted and summed, so the per-expert term is not recoverable
    from it. Pinning eager makes the arithmetic the observer records and
    the arithmetic the model runs the same arithmetic.

    The previous setting is restored on exit.
    """
    previous = getattr(model.config, "_experts_implementation", None)
    changed = previous is not None and previous != "eager"
    if previous != "eager":
        model.set_experts_implementation("eager")
    try:
        yield
    finally:
        if changed:
            model.set_experts_implementation(previous)
