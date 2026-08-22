"""REAP saliency scores and the expert-selection rule.

THE FORMULA (mirrored from upstream, cited line by line)
--------------------------------------------------------
`CerebrasResearch/reap@1970473c src/reap/pruning_metrics.py`:

    line 172   routing_weights = softmax(router_logits, dim=1)
    line 187   active_mask     = (selected_experts == i).any(dim=-1)
    line 193   ean_norm        = ||activations[i, active_mask, :]||_2
    line 198   reap[i]         = (ean_norm * routing_weights[active, i]).mean()
    line 209   the per-batch mean is folded into a token-count-weighted
               running mean, whose fixed point is  sum / count

so, over a whole calibration set,

    reap[e] = ( sum over tokens routed to e of ||expert_e(x_t)|| * g_{t,e} )
              / ( number of tokens routed to e )

which is exactly `weighted_ean_sum[e] / expert_frequency[e]`. Experts the
router never selected keep a score of 0 and are pruned first.

THE KEEP-COUNT RULE (pinned)
----------------------------
Default `rounding="floor"` reproduces upstream exactly — `reap/prune.py:261`
does `n_experts_to_prune = int(total_experts * compression_ratio)`, i.e.
truncation:

    n_prune = floor(E * c)        keep = E - n_prune
    E=8,   c=0.48 -> prune 3   -> keep 5
    E=256, c=0.48 -> prune 122 -> keep 134

`rounding="ceil"` (`n_prune = ceil(E * c)`) gives 133 of 256 at c=0.48,
which is the count crucible-labs published for their REAP-48
Qwen3.6-35B-A3B. It gives 4 of 8 on the miniature model.

**No single rule yields both 5-of-8 and 133-of-256**: keeping 5 of 8 needs
the prune count rounded DOWN (3.84 -> 3) while 133 of 256 needs it rounded
UP (122.88 -> 123). Both counts are pinned by tests, each under the rule
that produces it. The default is the upstream-faithful one; pass
`--rounding ceil` (or `--n-prune 123`) on the rental run to match
crucible-labs bit for bit.

Selection is per layer and uniform: the same number of experts survives in
every layer, because `config.num_experts` is a single scalar shared by all
layers of this architecture. Ties break towards pruning the lower index —
a stable ascending sort, where upstream's `torch.topk(..., largest=False)`
leaves ties implementation-defined.
"""

from __future__ import annotations

import math

import torch

from .observer import LayerSaliencyState

REAP_FORMULA_REF = ("CerebrasResearch/reap@1970473c "
                    "src/reap/pruning_metrics.py:172-211")
KEEP_RULE_REF = "CerebrasResearch/reap@1970473c src/reap/prune.py:261"

ROUNDINGS = ("floor", "ceil")
METRICS = ("reap", "ean_mean", "expert_frequency")


def _safe_divide(numerator: torch.Tensor, counts: torch.Tensor) -> torch.Tensor:
    counts = counts.to(torch.float64)
    return torch.where(counts > 0, numerator / counts.clamp(min=1.0),
                       torch.zeros_like(numerator))


def reap_scores(state: LayerSaliencyState) -> torch.Tensor:
    """Router-weighted expert activation norm, averaged over routed tokens."""
    return _safe_divide(state.weighted_ean_sum, state.expert_frequency)


def ean_mean_scores(state: LayerSaliencyState) -> torch.Tensor:
    """Unweighted mean activation norm (REAP's `ean_mean` ablation)."""
    return _safe_divide(state.ean_sum, state.expert_frequency)


def expert_probability(state: LayerSaliencyState) -> torch.Tensor:
    """Routing frequency over total tokens (`reap/prune.py:58-61`).

    With top_k > 1 this sums to top_k, not 1 — upstream's definition.
    """
    if state.total_tokens <= 0:
        return torch.zeros(state.num_experts, dtype=torch.float64)
    return state.expert_frequency.to(torch.float64) / float(state.total_tokens)


_METRIC_FNS = {
    "reap": reap_scores,
    "ean_mean": ean_mean_scores,
    "expert_frequency": expert_probability,
}


def saliency_matrix(states: dict[int, LayerSaliencyState],
                    metric: str = "reap") -> dict[int, torch.Tensor]:
    """Per-layer saliency vectors for the requested metric."""
    if metric not in METRICS:
        raise ValueError(f"unknown saliency metric {metric!r}; "
                         f"expected one of {METRICS}")
    fn = _METRIC_FNS[metric]
    return {layer: fn(state) for layer, state in sorted(states.items())}


def keep_count(num_experts: int, compression_ratio: float,
               rounding: str = "floor") -> int:
    """How many experts survive. See the module docstring for the rule."""
    if num_experts < 1:
        raise ValueError(f"num_experts must be >= 1, got {num_experts}")
    if not 0.0 <= compression_ratio < 1.0:
        raise ValueError(
            f"compression_ratio must be in [0, 1), got {compression_ratio}")
    if rounding not in ROUNDINGS:
        raise ValueError(f"unknown rounding {rounding!r}; "
                         f"expected one of {ROUNDINGS}")
    scaled = num_experts * compression_ratio
    n_prune = math.floor(scaled) if rounding == "floor" else math.ceil(scaled)
    return max(1, num_experts - n_prune)


def select_experts_to_prune(saliency: dict[int, torch.Tensor],
                            compression_ratio: float,
                            mode: str = "per_layer",
                            rounding: str = "floor") -> dict[int, list[int]]:
    """Per layer, the ascending indices of the experts to **keep**.

    Despite the upstream-matching name, the return value is the retained
    set — that is what `prune_model` consumes, and it is the thing worth
    recording in the checkpoint's provenance.
    """
    if mode == "global":
        raise NotImplementedError(
            "global (cross-layer) budgeting is not implemented: this "
            "architecture stores a single `config.num_experts` for all "
            "layers, so a non-uniform budget needs a config change too")
    if mode != "per_layer":
        raise ValueError(f"unknown mode {mode!r}; expected 'per_layer'")
    if not saliency:
        raise ValueError("no layers to select from")

    counts = {int(scores.shape[0]) for scores in saliency.values()}
    if len(counts) != 1:
        raise ValueError(f"layers disagree on the expert count: {sorted(counts)}")
    num_experts = counts.pop()
    keep = keep_count(num_experts, compression_ratio, rounding=rounding)
    n_prune = num_experts - keep

    selection: dict[int, list[int]] = {}
    for layer, scores in sorted(saliency.items()):
        order = torch.argsort(scores.to(torch.float64), stable=True)
        kept = order[n_prune:].tolist()
        selection[layer] = sorted(int(i) for i in kept)
    return selection


def metrics_help() -> str:
    """`--metric` help text, kept next to the metric table it describes."""
    return ("saliency metric: 'reap' (router-weighted activation norm, the "
            "REAP score), 'ean_mean' (unweighted activation norm), or "
            "'expert_frequency' (routing frequency)")


def saliency_quantiles(scores: torch.Tensor) -> dict[str, float]:
    """Compact distribution summary for the run report."""
    values = scores.to(torch.float64).flatten()
    q = torch.quantile(values, torch.tensor([0.10, 0.50, 0.90],
                                            dtype=torch.float64))
    return {
        "min": float(values.min()),
        "p10": float(q[0]),
        "p50": float(q[1]),
        "p90": float(q[2]),
        "max": float(values.max()),
        "mean": float(values.mean()),
    }
