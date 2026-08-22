"""Tests for tools.flywheel.prune.saliency — the REAP formulas and the
expert-selection rule.

The saliency math mirrors upstream REAP
(CerebrasResearch/reap@1970473c, src/reap/pruning_metrics.py:186-198):

    reap[e] = mean over calibration tokens routed to e of
              ( ||expert_e(x_t)||_2  *  softmax(router_logits)[t, e] )

The keep-count rule is pinned here and nowhere else. See the module
docstring of saliency.py for why the default is `floor`.
"""

import unittest

try:  # pragma: no cover - environment guard
    import torch  # noqa: F401
    import transformers  # noqa: F401
except ImportError as exc:  # pragma: no cover
    raise unittest.SkipTest(f"torch/transformers unavailable: {exc}")

import torch

from tools.flywheel.prune import PruneConfigurationError
from tools.flywheel.prune import saliency as sal
from tools.flywheel.prune.observer import LayerSaliencyState


def _state(num_experts, freq, weighted_sum, ean_sum=None, total_tokens=100):
    return LayerSaliencyState(
        num_experts=num_experts,
        top_k=2,
        total_tokens=total_tokens,
        expert_frequency=torch.tensor(freq, dtype=torch.long),
        ean_sum=torch.tensor(
            ean_sum if ean_sum is not None else weighted_sum, dtype=torch.float64
        ),
        weighted_ean_sum=torch.tensor(weighted_sum, dtype=torch.float64),
        weighted_expert_frequency_sum=torch.zeros(num_experts, dtype=torch.float64),
        max_activations=torch.zeros(num_experts, dtype=torch.float32),
    )


class KeepCountRuleTest(unittest.TestCase):
    """The rounding rule is a pinned constant of this tool, not a taste."""

    def test_floor_rule_keeps_five_of_eight_at_048(self):
        self.assertEqual(sal.keep_count(8, 0.48), 5)

    def test_floor_rule_keeps_134_of_256_at_048(self):
        # Same rule, real geometry: floor(256 * 0.48) = 122 pruned -> 134 kept.
        self.assertEqual(sal.keep_count(256, 0.48), 134)

    def test_ceil_rule_keeps_133_of_256_at_048(self):
        # crucible-labs' published REAP-48 kept 133/256, which is the `ceil`
        # rule (ceil(256*0.48) = 123 pruned). Offered so the rental run can
        # reproduce their count exactly.
        self.assertEqual(sal.keep_count(256, 0.48, rounding="ceil"), 133)

    def test_ceil_rule_keeps_four_of_eight_at_048(self):
        self.assertEqual(sal.keep_count(8, 0.48, rounding="ceil"), 4)

    def test_zero_compression_keeps_everything(self):
        self.assertEqual(sal.keep_count(256, 0.0), 256)

    def test_at_least_one_expert_survives(self):
        self.assertEqual(sal.keep_count(8, 0.99), 1)

    def test_rejects_out_of_range_compression(self):
        for bad in (-0.1, 1.0, 1.5):
            with self.assertRaises(ValueError):
                sal.keep_count(8, bad)

    def test_rejects_unknown_rounding(self):
        with self.assertRaises(ValueError):
            sal.keep_count(8, 0.5, rounding="banker")


class RoutableGuardTest(unittest.TestCase):
    """A layer that keeps fewer experts than top-k cannot route at all."""

    def test_refuses_to_keep_fewer_experts_than_top_k(self):
        with self.assertRaises(PruneConfigurationError) as ctx:
            sal.keep_count(8, 0.98, num_experts_per_tok=2)
        message = str(ctx.exception)
        self.assertIn("keep 1 of 8", message)
        self.assertIn("num_experts_per_tok=2", message)
        self.assertIn("0.75", message)  # the stated ceiling: (8-2)/8

    def test_allows_keeping_exactly_top_k(self):
        self.assertEqual(sal.keep_count(8, 0.75, num_experts_per_tok=2), 2)

    def test_real_geometry_at_048_is_far_above_top_k(self):
        self.assertEqual(sal.keep_count(256, 0.48, num_experts_per_tok=8), 134)

    def test_guard_is_off_when_top_k_is_not_supplied(self):
        self.assertEqual(sal.keep_count(8, 0.98), 1)

    def test_selection_refuses_the_same_case(self):
        with self.assertRaises(PruneConfigurationError):
            sal.select_experts_to_prune({0: torch.zeros(8)}, 0.98,
                                        num_experts_per_tok=2)


class ReapScoreTest(unittest.TestCase):
    def test_reap_is_weighted_norm_sum_over_routed_token_count(self):
        state = _state(4, [10, 5, 2, 0], [20.0, 5.0, 1.0, 0.0])
        scores = sal.reap_scores(state)
        torch.testing.assert_close(
            scores, torch.tensor([2.0, 1.0, 0.5, 0.0], dtype=torch.float64)
        )

    def test_never_routed_expert_scores_zero(self):
        state = _state(3, [4, 0, 1], [8.0, 0.0, 3.0])
        self.assertEqual(sal.reap_scores(state)[1].item(), 0.0)

    def test_ean_mean_is_unweighted_norm_mean(self):
        state = _state(3, [4, 0, 2], [0.0, 0.0, 0.0], ean_sum=[8.0, 0.0, 3.0])
        torch.testing.assert_close(
            sal.ean_mean_scores(state),
            torch.tensor([2.0, 0.0, 1.5], dtype=torch.float64),
        )

    def test_expert_probability_normalises_by_total_tokens(self):
        state = _state(2, [30, 10], [0.0, 0.0], total_tokens=20)
        torch.testing.assert_close(
            sal.expert_probability(state),
            torch.tensor([1.5, 0.5], dtype=torch.float64),
        )

    def test_saliency_matrix_covers_every_layer(self):
        states = {0: _state(3, [1, 1, 1], [1.0, 2.0, 3.0]),
                  2: _state(3, [1, 1, 1], [3.0, 2.0, 1.0])}
        matrix = sal.saliency_matrix(states)
        self.assertEqual(sorted(matrix), [0, 2])
        self.assertEqual(matrix[0].shape, (3,))

    def test_saliency_matrix_rejects_unknown_metric(self):
        with self.assertRaises(ValueError):
            sal.saliency_matrix({0: _state(2, [1, 1], [1.0, 1.0])}, metric="vibes")


class SelectionTest(unittest.TestCase):
    def test_keeps_the_highest_saliency_experts(self):
        saliency = {0: torch.tensor([0.1, 0.9, 0.5, 0.7, 0.2, 0.8, 0.3, 0.6])}
        keep = sal.select_experts_to_prune(saliency, 0.48)
        # keep 5 of 8: the five largest are 0.9(1) 0.8(5) 0.7(3) 0.6(7) 0.5(2)
        self.assertEqual(keep[0], [1, 2, 3, 5, 7])

    def test_kept_indices_are_sorted_ascending(self):
        saliency = {0: torch.tensor([9.0, 1.0, 8.0, 2.0, 7.0, 3.0, 6.0, 4.0])}
        keep = sal.select_experts_to_prune(saliency, 0.48)
        self.assertEqual(keep[0], sorted(keep[0]))

    def test_ties_break_towards_the_lower_index_being_pruned(self):
        # All equal -> the four highest indices survive (stable ascending sort,
        # prune from the front). Deterministic, unlike torch.topk on ties.
        saliency = {0: torch.zeros(8)}
        keep = sal.select_experts_to_prune(saliency, 0.48)
        self.assertEqual(keep[0], [3, 4, 5, 6, 7])

    def test_layers_are_selected_independently(self):
        saliency = {
            0: torch.tensor([1.0, 2.0, 3.0, 4.0]),
            1: torch.tensor([4.0, 3.0, 2.0, 1.0]),
        }
        keep = sal.select_experts_to_prune(saliency, 0.5)
        self.assertEqual(keep[0], [2, 3])
        self.assertEqual(keep[1], [0, 1])

    def test_ceil_rounding_flows_through_selection(self):
        saliency = {0: torch.tensor([0.1, 0.9, 0.5, 0.7, 0.2, 0.8, 0.3, 0.6])}
        keep = sal.select_experts_to_prune(saliency, 0.48, rounding="ceil")
        self.assertEqual(keep[0], [1, 3, 5, 7])

    def test_global_mode_is_refused_not_silently_per_layer(self):
        with self.assertRaises(NotImplementedError):
            sal.select_experts_to_prune({0: torch.zeros(8)}, 0.5, mode="global")

    def test_rejects_ragged_expert_counts(self):
        saliency = {0: torch.zeros(8), 1: torch.zeros(6)}
        with self.assertRaises(ValueError):
            sal.select_experts_to_prune(saliency, 0.5)

    def test_quantiles_are_reported_for_the_summary(self):
        q = sal.saliency_quantiles(torch.arange(101, dtype=torch.float64))
        self.assertAlmostEqual(q["min"], 0.0)
        self.assertAlmostEqual(q["p50"], 50.0)
        self.assertAlmostEqual(q["max"], 100.0)


if __name__ == "__main__":
    unittest.main()
