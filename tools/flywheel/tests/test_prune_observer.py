"""Tests for tools.flywheel.prune.observer — the REAP expert-saliency
observer for `qwen3_5_moe`.

The load-bearing test is `DenseReferenceTest`: it recomputes REAP's
saliency the way upstream does it (dense — every expert applied to every
token, then masked; src/reap/observer.py:374-382 + pruning_metrics.py:186-198)
and asserts the observer's sparse accumulation agrees. That is the proof
that skipping the dense pass changes cost, not math.
"""

import unittest

try:  # pragma: no cover - environment guard
    import torch  # noqa: F401
    import transformers  # noqa: F401
except ImportError as exc:  # pragma: no cover
    raise unittest.SkipTest(f"torch/transformers unavailable: {exc}")

import torch
import torch.nn.functional as F

from tools.flywheel.prune.blocks import find_moe_blocks
from tools.flywheel.prune.observer import ExpertSaliencyObserver
from tools.flywheel.prune import saliency as sal
from tools.flywheel.tests.prune_fixture import (
    NUM_EXPERTS,
    build_mini_model,
    mini_input_ids,
)


def _dense_reference(model, input_ids, attention_mask=None):
    """Upstream REAP's own formula, computed densely, from captured inputs.

    Mirrors `reap/observer.py::_hook_factory` (loop branch) feeding
    `reap/pruning_metrics.py::update_pruning_state`.
    """
    captured = {}

    def make(idx):
        def hook(module, args, output):
            captured[idx] = (args[0].detach().reshape(-1, module.hidden_dim),
                             output[2].detach())
        return hook

    handles = [ref.gate.register_forward_hook(make(ref.layer_index))
               for ref in find_moe_blocks(model)]
    try:
        with torch.no_grad():
            model(input_ids=input_ids, attention_mask=attention_mask)
    finally:
        for h in handles:
            h.remove()

    out = {}
    for ref in find_moe_blocks(model):
        x, selected = captured[ref.layer_index]
        experts = ref.experts
        num_experts = ref.num_experts
        if attention_mask is not None:
            keep = attention_mask.reshape(-1).bool()
            x, selected = x[keep], selected[keep]
        raw_logits = F.linear(x, ref.gate.weight)
        routing_weights = F.softmax(raw_logits, dim=1, dtype=torch.float)
        # dense: every expert on every token (this is the wasteful part)
        activations = torch.zeros((num_experts, *x.shape))
        with torch.no_grad():
            for e in range(num_experts):
                gate_up = F.linear(x, experts.gate_up_proj[e])
                g, u = gate_up.chunk(2, dim=-1)
                activations[e] = F.linear(experts.act_fn(g) * u,
                                          experts.down_proj[e])
        reap = torch.zeros(num_experts, dtype=torch.float64)
        ean_sum = torch.zeros(num_experts, dtype=torch.float64)
        weighted = torch.zeros(num_experts, dtype=torch.float64)
        freq = torch.bincount(selected.reshape(-1), minlength=num_experts)
        for e in range(num_experts):
            active = (selected == e).any(dim=-1)
            if not active.any():
                continue
            norms = torch.linalg.norm(activations[e, active, :], dim=-1)
            w = routing_weights[active, e]
            ean_sum[e] = norms.sum().double()
            weighted[e] = (norms * w).sum().double()
            reap[e] = (norms * w).mean().double()
        out[ref.layer_index] = {
            "reap": reap,
            "ean_sum": ean_sum,
            "weighted_ean_sum": weighted,
            "expert_frequency": freq,
            "total_tokens": int(x.shape[0]),
        }
    return out


def _run_capturing_routing(model, input_ids):
    """Logits plus the per-layer top-k routing indices for one forward."""
    routing = {}

    def make(idx):
        def hook(module, args, output):
            routing[idx] = output[2].detach().clone()
        return hook

    handles = [ref.gate.register_forward_hook(make(ref.layer_index))
               for ref in find_moe_blocks(model)]
    try:
        with torch.no_grad():
            logits = model(input_ids=input_ids).logits.clone()
    finally:
        for handle in handles:
            handle.remove()
    return logits, routing


class NonPerturbationTest(unittest.TestCase):
    """What the observer does and does not change.

    The hooks return `None`, so they cannot perturb the forward pass. The
    one visible effect of *installing* the observer is that it pins the
    expert kernel to eager for the calibration pass (see
    `blocks.eager_experts`). Both halves are asserted: exact equality when
    the model is already eager, and `allclose` + identical routing when it
    is not.
    """

    def test_hooked_forward_is_exact_when_the_model_is_already_eager(self):
        model = build_mini_model()  # pinned eager: the switch is a no-op
        ids = mini_input_ids()
        with torch.no_grad():
            clean = model(input_ids=ids).logits.clone()
        observer = ExpertSaliencyObserver(model)
        with observer:
            with torch.no_grad():
                hooked = model(input_ids=ids).logits.clone()
        self.assertTrue(torch.equal(clean, hooked),
                        f"max delta {(clean - hooked).abs().max().item()}")

    def test_grouped_mm_model_is_switched_to_eager_and_restored(self):
        model = build_mini_model(pin_eager=False)
        if model.config._experts_implementation != "grouped_mm":
            self.skipTest("this box does not dispatch grouped_mm; "
                          "nothing to compare eager against")
        ids = mini_input_ids()
        clean_logits, clean_routing = _run_capturing_routing(model, ids)

        observer = ExpertSaliencyObserver(model)
        with observer:
            self.assertEqual(model.config._experts_implementation, "eager")
            hooked_logits, hooked_routing = _run_capturing_routing(model, ids)
        self.assertEqual(model.config._experts_implementation, "grouped_mm")

        # The kernel swap is worth order 1e-7 on the logits (measured
        # 1.19e-7 max abs delta), not zero — the honest claim.
        delta = (clean_logits - hooked_logits).abs().max().item()
        self.assertLess(delta, 1e-5, f"kernel swap moved logits by {delta}")
        # Routing is unaffected at that scale: same experts, same order.
        self.assertEqual(sorted(clean_routing), sorted(hooked_routing))
        for layer, indices in clean_routing.items():
            self.assertTrue(torch.equal(indices, hooked_routing[layer]),
                            f"layer {layer} routing changed")

    def test_none_experts_implementation_is_restored_as_none(self):
        model = build_mini_model()
        model.config._experts_implementation = None
        observer = ExpertSaliencyObserver(model)
        with observer:
            self.assertEqual(model.config._experts_implementation, "eager")
        self.assertIsNone(model.config._experts_implementation)

    def test_hooks_are_removed_on_exit(self):
        model = build_mini_model()
        observer = ExpertSaliencyObserver(model)
        with observer:
            self.assertEqual(len(observer.handles), 4)
        self.assertEqual(observer.handles, [])

    def test_experts_implementation_is_forced_eager_then_restored(self):
        model = build_mini_model()
        model.set_experts_implementation("grouped_mm")
        observer = ExpertSaliencyObserver(model)
        with observer:
            self.assertEqual(model.config._experts_implementation, "eager")
        self.assertEqual(model.config._experts_implementation, "grouped_mm")

    def test_every_moe_layer_is_observed(self):
        model = build_mini_model()
        observer = ExpertSaliencyObserver(model)
        with observer, torch.no_grad():
            model(input_ids=mini_input_ids())
        self.assertEqual(sorted(observer.state), [0, 1, 2, 3])
        for st in observer.state.values():
            self.assertEqual(st.num_experts, NUM_EXPERTS)
            self.assertEqual(st.top_k, 2)


class DenseReferenceTest(unittest.TestCase):
    def test_matches_upstream_dense_formula(self):
        model = build_mini_model()
        ids = mini_input_ids(seq=16)
        expected = _dense_reference(model, ids)

        observer = ExpertSaliencyObserver(model)
        with observer, torch.no_grad():
            model(input_ids=ids)

        for layer, want in expected.items():
            got = observer.state[layer]
            self.assertEqual(got.total_tokens, want["total_tokens"], layer)
            torch.testing.assert_close(got.expert_frequency,
                                       want["expert_frequency"], msg=str(layer))
            torch.testing.assert_close(got.ean_sum, want["ean_sum"],
                                       rtol=1e-6, atol=1e-6, msg=str(layer))
            torch.testing.assert_close(got.weighted_ean_sum,
                                       want["weighted_ean_sum"],
                                       rtol=1e-6, atol=1e-6, msg=str(layer))
            torch.testing.assert_close(sal.reap_scores(got), want["reap"],
                                       rtol=1e-6, atol=1e-6, msg=str(layer))

    def test_attention_mask_excludes_padding_tokens(self):
        model = build_mini_model()
        ids = mini_input_ids(seq=16)
        mask = torch.ones_like(ids)
        mask[:, 10:] = 0
        observer = ExpertSaliencyObserver(model)
        with observer:
            with observer.set_attention_mask(mask), torch.no_grad():
                model(input_ids=ids, attention_mask=mask)
        for st in observer.state.values():
            self.assertEqual(st.total_tokens, 10)
            self.assertEqual(int(st.expert_frequency.sum()), 10 * 2)


class SaliencySanityTest(unittest.TestCase):
    def test_unrouted_experts_have_zero_saliency(self):
        # One token, top-2 of 8 -> six experts at layer 0 are never routed.
        model = build_mini_model()
        observer = ExpertSaliencyObserver(model)
        with observer, torch.no_grad():
            model(input_ids=mini_input_ids(seq=1))
        st = observer.state[0]
        scores = sal.reap_scores(st)
        unrouted = (st.expert_frequency == 0).nonzero().flatten()
        self.assertEqual(len(unrouted), NUM_EXPERTS - 2)
        self.assertTrue(torch.all(scores[unrouted] == 0.0))

    def test_amplified_expert_becomes_the_most_salient(self):
        model = build_mini_model()
        ids = mini_input_ids(seq=16)
        observer = ExpertSaliencyObserver(model)
        with observer, torch.no_grad():
            model(input_ids=ids)
        st = observer.state[0]
        target = int(torch.argmax(st.expert_frequency))
        self.assertGreater(int(st.expert_frequency[target]), 0)

        block = find_moe_blocks(model)[0].block
        with torch.no_grad():
            block.experts.down_proj[target] *= 1000.0
        boosted = ExpertSaliencyObserver(model)
        with boosted, torch.no_grad():
            model(input_ids=ids)
        # Layer 0's routing cannot change (it depends only on layer-0 input),
        # so the only change is this expert's output magnitude.
        scores = sal.reap_scores(boosted.state[0])
        self.assertEqual(int(torch.argmax(scores)), target)
        others = torch.cat([scores[:target], scores[target + 1:]])
        self.assertGreater(scores[target].item(), 100 * others.max().item())

    def test_repeated_runs_are_bit_identical(self):
        ids = mini_input_ids(seq=16)
        results = []
        for _ in range(2):
            model = build_mini_model()
            observer = ExpertSaliencyObserver(model)
            with observer, torch.no_grad():
                model(input_ids=ids)
            results.append(sal.reap_scores(observer.state[2]))
        self.assertTrue(torch.equal(results[0], results[1]))

    def test_accumulates_across_batches(self):
        model = build_mini_model()
        ids = mini_input_ids(seq=8)
        observer = ExpertSaliencyObserver(model)
        with observer, torch.no_grad():
            model(input_ids=ids)
            model(input_ids=ids)
        self.assertEqual(observer.state[0].total_tokens, 16)

    def test_renormalised_weights_are_larger_than_raw_softmax(self):
        ids = mini_input_ids(seq=16)
        plain, renorm = [], []
        for target, flag in ((plain, False), (renorm, True)):
            model = build_mini_model()
            observer = ExpertSaliencyObserver(
                model, renormalize_router_weights=flag)
            with observer, torch.no_grad():
                model(input_ids=ids)
            target.append(sal.reap_scores(observer.state[0]))
        routed = plain[0] > 0
        self.assertTrue(torch.all(renorm[0][routed] > plain[0][routed]))


if __name__ == "__main__":
    unittest.main()
