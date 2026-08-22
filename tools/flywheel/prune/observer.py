"""ExpertSaliencyObserver — REAP expert-saliency statistics for `qwen3_5_moe`.

HOW IT REACHES THE PER-EXPERT CONTRIBUTION
------------------------------------------
`Qwen3_5MoeSparseMoeBlock.forward` returns a single already-combined
Tensor and never exposes router logits or per-expert outputs
(modeling_qwen3_5_moe.py:781-792). Upstream REAP's fused branch
(reap/observer.py:353-372) assumes `output` is `(hidden, router_scores)`,
calls `module.router(x)` and `module.experts(x)` — none of which exist
here. So the block's own output is unusable.

Instead we register a **forward hook on the block's router**
(`block.gate`, class `Qwen3_5MoeTopKRouter`). That single hook yields
everything routing-related straight from the model's own computation:

    args[0] -> hidden_states, already flattened to (T, H) by the block
    output  -> (probs, scores, indices) from modeling:762-770, where
               probs   = softmax(F.linear(x, gate.weight))  (T, E) float32
               scores  = top-k probs renormalised to sum 1  (T, K)
               indices = the selected experts                (T, K)

`probs` is exactly REAP's `F.softmax(router_logits, dim=1)`
(pruning_metrics.py:172), taken from the model rather than recomputed.
NOTE the naming trap: this router's *returned* `router_logits` is already
softmaxed (modeling:765). Feeding it to upstream's
`update_pruning_state` would softmax it twice.

The per-expert contribution is then replayed from the experts' own
parameters, mirroring the eager kernel body line for line
(modeling_qwen3_5_moe.py:745-747):

    gate, up = F.linear(x_t, gate_up_proj[e]).chunk(2, -1)
    out      = F.linear(act_fn(gate) * up, down_proj[e])

exclusive of line 748's `* top_k_weights`, because REAP records the
*unweighted* activation ("we do not apply router_scores",
reap/observer.py:369-372) and applies the router weight itself.

WHAT INSTALLING THE OBSERVER DOES AND DOES NOT CHANGE
-----------------------------------------------------
The **hooks** cannot perturb anything: they return `None`, so the forward
pass is untouched. **Installing the observer** does change one thing — it
pins the expert kernel to eager for the calibration pass (see
`blocks.eager_experts` for why it must). Against a model already running
eager that is a no-op and the logits are bit-identical; against the
default `grouped_mm` it swaps kernels, which moves the model's own logits
by order 1e-7 (measured 1.19e-7 max abs delta on the miniature model).
Routing indices are identical at that scale, and the previous kernel —
`None` included — is restored on exit.

Both halves are asserted in `test_prune_observer.py::NonPerturbationTest`:
exact `torch.equal` for the eager-pinned case, and `allclose` at
`atol=1e-5` plus identical top-k routing plus kernel restoration for the
grouped_mm case.

DELIBERATE DIVERGENCES FROM UPSTREAM (same math, different cost)
---------------------------------------------------------------
1. **Sparse, not dense.** Upstream materialises every expert's output for
   every token — `(E, T, H)` — then masks down to the routed rows
   (reap/observer.py:374-382). Every pruning metric in
   `update_pruning_state` reads only `activations[i, active_mask, :]`, so
   the unrouted rows are pure waste. We evaluate expert `e` on the tokens
   routed to `e` only: a factor of E/top_k less expert compute (32x at
   256/8). Equivalence is asserted against a dense reference in
   `test_prune_observer.py::DenseReferenceTest`.
2. **Exact float64 accumulators, not float32 Welford.** Upstream tracks
   `reap` with an `OnlineStatsTracker` (Welford + Kahan, float32) whose
   fixed point is `sum / count`; we accumulate that sum and count in
   float64 directly. Same estimator, no batch-order dependence.
3. **Pruning metrics only.** The merge-only statistics (`ttm_similarity`,
   characteristic activations, `pairwise_expert_frequency`) are not
   recorded — this tool prunes, it does not merge. Equivalent to upstream's
   `record_pruning_metrics_only=True`.
"""

from __future__ import annotations

from contextlib import contextmanager
from dataclasses import dataclass, field

import torch
import torch.nn.functional as F

from .blocks import MoEBlockRef, eager_experts, find_moe_blocks


@dataclass
class LayerSaliencyState:
    """Per-layer accumulators; the pruning subset of REAP's layer state
    (`reap/pruning_metrics.py::initialize_pruning_state`, lines 22-64)."""

    num_experts: int
    top_k: int
    total_tokens: int = 0
    expert_frequency: torch.Tensor = field(default=None)
    ean_sum: torch.Tensor = field(default=None)
    weighted_ean_sum: torch.Tensor = field(default=None)
    weighted_expert_frequency_sum: torch.Tensor = field(default=None)
    max_activations: torch.Tensor = field(default=None)

    def __post_init__(self) -> None:
        e = self.num_experts
        if self.expert_frequency is None:
            self.expert_frequency = torch.zeros(e, dtype=torch.long)
        if self.ean_sum is None:
            self.ean_sum = torch.zeros(e, dtype=torch.float64)
        if self.weighted_ean_sum is None:
            self.weighted_ean_sum = torch.zeros(e, dtype=torch.float64)
        if self.weighted_expert_frequency_sum is None:
            self.weighted_expert_frequency_sum = torch.zeros(e, dtype=torch.float64)
        if self.max_activations is None:
            self.max_activations = torch.zeros(e, dtype=torch.float32)


def expert_activation(experts, index: int, hidden: torch.Tensor) -> torch.Tensor:
    """One routed expert's raw output, before the router weight is applied.

    Mirrors `Qwen3_5MoeExperts.forward` lines 745-747 exactly; line 748's
    `* top_k_weights[...]` is deliberately omitted (see module docstring).
    """
    gate_up = F.linear(hidden, experts.gate_up_proj[index])
    gate, up = gate_up.chunk(2, dim=-1)
    return F.linear(experts.act_fn(gate) * up, experts.down_proj[index])


class ExpertSaliencyObserver:
    """Accumulates REAP saliency statistics over a calibration pass.

    Usage:
        observer = ExpertSaliencyObserver(model)
        with observer:
            for batch in calibration:
                with observer.set_attention_mask(batch["attention_mask"]):
                    model(**batch)
        scores = saliency.saliency_matrix(observer.state)
    """

    def __init__(self, model, *, renormalize_router_weights: bool = False,
                 force_eager: bool = True) -> None:
        self.model = model
        self.renormalize_router_weights = renormalize_router_weights
        self.force_eager = force_eager
        self.blocks: list[MoEBlockRef] = find_moe_blocks(model)
        self.state: dict[int, LayerSaliencyState] = {}
        self.handles: list = []
        self._attention_mask = None
        self._eager_ctx = None

    # -- lifecycle ---------------------------------------------------------
    def install(self) -> "ExpertSaliencyObserver":
        if self.handles:
            raise RuntimeError("observer hooks are already installed")
        if self.force_eager:
            self._eager_ctx = eager_experts(self.model)
            self._eager_ctx.__enter__()
        for ref in self.blocks:
            self.handles.append(
                ref.gate.register_forward_hook(self._make_hook(ref)))
        return self

    def remove(self) -> None:
        for handle in self.handles:
            handle.remove()
        self.handles = []
        if self._eager_ctx is not None:
            self._eager_ctx.__exit__(None, None, None)
            self._eager_ctx = None

    def __enter__(self) -> "ExpertSaliencyObserver":
        return self.install()

    def __exit__(self, *exc) -> None:
        self.remove()

    @contextmanager
    def set_attention_mask(self, attention_mask):
        """Exclude padding tokens from the statistics for one forward pass."""
        previous = self._attention_mask
        self._attention_mask = attention_mask
        try:
            yield
        finally:
            self._attention_mask = previous

    def reset(self) -> None:
        self.state = {}

    # -- the hook ----------------------------------------------------------
    def _make_hook(self, ref: MoEBlockRef):
        layer = ref.layer_index
        experts = ref.experts

        @torch.no_grad()
        def hook(module, args, output):
            probs, _scores, indices = output
            hidden = args[0].reshape(-1, module.hidden_dim).detach()
            probs = probs.detach().float()
            indices = indices.detach()

            mask = self._attention_mask
            if mask is not None:
                flat = mask.reshape(-1).bool().to(hidden.device)
                hidden, probs, indices = hidden[flat], probs[flat], indices[flat]

            num_experts = int(module.num_experts)
            state = self.state.get(layer)
            if state is None:
                state = LayerSaliencyState(num_experts=num_experts,
                                           top_k=int(module.top_k))
                self.state[layer] = state
            if state.num_experts != num_experts:
                raise RuntimeError(
                    f"layer {layer} expert count changed mid-calibration: "
                    f"{state.num_experts} -> {num_experts}")

            weights = probs
            if self.renormalize_router_weights and indices.numel():
                # reap/pruning_metrics.py:175-184 — divide the whole row by
                # the top-k mass, i.e. the weight the block actually applies
                # (modeling_qwen3_5_moe.py:767).
                topk_mass = torch.gather(probs, 1, indices).sum(dim=-1,
                                                                keepdim=True)
                weights = torch.clamp(probs / topk_mass,
                                      min=torch.finfo(probs.dtype).eps)

            self._accumulate(state, experts, hidden, probs, weights, indices,
                             num_experts)

        return hook

    @staticmethod
    def _accumulate(state, experts, hidden, probs, weights, indices,
                    num_experts) -> None:
        state.total_tokens += int(hidden.shape[0])
        if not indices.numel():
            return
        # reap/pruning_metrics.py:118-120
        state.expert_frequency += torch.bincount(
            indices.reshape(-1), minlength=num_experts).cpu()

        expert_mask = F.one_hot(indices, num_classes=num_experts).permute(2, 1, 0)
        hit = torch.greater(expert_mask.sum(dim=(-1, -2)), 0).nonzero()
        for entry in hit:
            index = int(entry[0])
            _, token_idx = torch.where(expert_mask[index])
            token_idx = torch.unique(token_idx)
            activation = expert_activation(experts, index, hidden[token_idx])
            # reap/pruning_metrics.py:193-198
            norms = torch.linalg.norm(activation.float(), dim=-1)
            routed_weights = weights[token_idx, index]
            state.ean_sum[index] += norms.sum().double().cpu()
            state.weighted_ean_sum[index] += (
                (norms * routed_weights).sum().double().cpu())
            state.weighted_expert_frequency_sum[index] += (
                routed_weights.sum().double().cpu())
            # reap/pruning_metrics.py:200-202
            batch_max = activation.float().max().cpu()
            if batch_max > state.max_activations[index]:
                state.max_activations[index] = batch_max

    # -- reporting ---------------------------------------------------------
    def to_dict(self) -> dict:
        """Plain-python snapshot, for saving next to a checkpoint."""
        return {
            layer: {
                "num_experts": st.num_experts,
                "top_k": st.top_k,
                "total_tokens": st.total_tokens,
                "expert_frequency": st.expert_frequency.tolist(),
                "ean_sum": st.ean_sum.tolist(),
                "weighted_ean_sum": st.weighted_ean_sum.tolist(),
                "weighted_expert_frequency_sum":
                    st.weighted_expert_frequency_sum.tolist(),
                "max_activations": st.max_activations.tolist(),
            }
            for layer, st in sorted(self.state.items())
        }
