# bloomery — an operating layer for local LLMs.
# Copyright (C) 2026 Brice Lancaster
#
# This program is free software: you can redistribute it and/or modify it
# under the terms of the GNU Affero General Public License, version 3, as
# published by the Free Software Foundation.
#
# This program is distributed in the hope that it will be useful, but WITHOUT
# ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
# FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
# for more details.
#
# You should have received a copy of the GNU Affero General Public License
# along with this program. If not, see <https://www.gnu.org/licenses/>.
#
# Commercial licensing is available as an alternative to the AGPL — see
# LICENSING.md.

import unittest

from openai_tools.session import Session
from openai_tools.template import ChatTemplate

TPL = "adapters/openai-tools/templates/qwen36-reap48-ours.jinja"
TOOLS = [{"type": "function", "function": {
    "name": "terminal", "parameters": {"type": "object", "properties": {}}}}]
U1 = {"role": "user", "content": "first"}
U2 = {"role": "user", "content": "second"}
GEN = "\nreasoning\n</think>\n\nan answer"
TC_A = [{"id": "call_1", "type": "function",
         "function": {"name": "terminal", "arguments": {"command": "ls"}}}]
TC_B = [{"id": "call_2", "type": "function",
         "function": {"name": "terminal", "arguments": {"command": "rm -rf /"}}}]


class SessionTest(unittest.TestCase):
    def setUp(self):
        self.s = Session("a1", ChatTemplate.load(TPL))

    def test_first_turn_sends_the_whole_render_including_tools(self):
        delta, reset = self.s.next_delta([U1], TOOLS)
        self.assertFalse(reset)
        self.assertIn("<tools>", delta)
        self.assertTrue(delta.endswith("<|im_start|>assistant\n<think>\n"))

    def test_second_turn_sends_only_the_new_turn_not_the_preamble(self):
        first, _ = self.s.next_delta([U1], TOOLS)
        self.s.record_generation(GEN)
        assistant = {"role": "assistant", "content": GEN}
        delta, reset = self.s.next_delta([U1, assistant, U2], TOOLS)
        self.assertFalse(reset)
        self.assertNotIn("<tools>", delta)          # the expensive part is NOT resent
        self.assertLess(len(delta), len(first) // 2)
        self.assertIn("second", delta)

    def test_every_delta_begins_at_a_special_token_boundary(self):
        self.s.next_delta([U1], TOOLS)
        self.s.record_generation(GEN)
        delta, _ = self.s.next_delta(
            [U1, {"role": "assistant", "content": GEN}, U2], TOOLS)
        self.assertTrue(delta.startswith("<|im_end|>\n<|im_start|>"))

    def test_a_rewritten_history_resets_instead_of_appending_onto_stale_kv(self):
        self.s.next_delta([U1], TOOLS)
        self.s.record_generation(GEN)
        # The client compressed: the first user turn is gone.
        delta, reset = self.s.next_delta([U2], TOOLS)
        self.assertTrue(reset)
        self.assertIn("<tools>", delta)   # a reset re-sends the full render

    def test_an_unchanged_history_is_not_treated_as_divergence(self):
        self.s.next_delta([U1], TOOLS)
        self.s.record_generation(GEN)
        _, reset = self.s.next_delta(
            [U1, {"role": "assistant", "content": GEN}, U2], TOOLS)
        self.assertFalse(reset)

    def test_default_keep_reasoning_is_true(self):
        self.assertTrue(self.s.keep_reasoning)

    def test_second_turn_resends_full_render_when_keep_reasoning_is_false(self):
        s = Session("a2", ChatTemplate.load(TPL), keep_reasoning=False)
        s.next_delta([U1], TOOLS)
        s.record_generation(GEN)
        assistant = {"role": "assistant", "content": GEN}
        delta, _ = s.next_delta([U1, assistant, U2], TOOLS)
        self.assertIn("<tools>", delta)   # option B: no reuse, full render every turn

    def test_declining_reuse_via_keep_reasoning_false_is_not_a_reset(self):
        # The client did nothing wrong here -- it's a legitimate append. We
        # are choosing not to reuse the KV, which is a deliberate design
        # tradeoff (spec's option B), not a divergence. A later reader must
        # not "fix" this by making it True.
        s = Session("a3", ChatTemplate.load(TPL), keep_reasoning=False)
        s.next_delta([U1], TOOLS)
        s.record_generation(GEN)
        assistant = {"role": "assistant", "content": GEN}
        _, reset = s.next_delta([U1, assistant, U2], TOOLS)
        self.assertFalse(reset)

    def test_a_rewritten_tool_call_turn_with_matching_empty_content_still_resets(self):
        # Finding 1: content alone is not enough identity for a tool-call
        # turn -- two different tool calls can both carry content "" (or
        # None). If _is_extension only compared role/content, this would be
        # misclassified as an append and the delta would be appended onto a
        # KV cache whose actual contents no longer match -- silent context
        # corruption. tool_calls must be part of the comparison.
        self.s.next_delta([U1], TOOLS)
        assistant_a = {"role": "assistant", "content": "", "tool_calls": TC_A}
        self.s.next_delta([U1, assistant_a, U2], TOOLS)
        # The client rewrote history: same position, same empty content,
        # a DIFFERENT tool call.
        assistant_b = {"role": "assistant", "content": "", "tool_calls": TC_B}
        delta, reset = self.s.next_delta([U1, assistant_b, U2], TOOLS)
        self.assertTrue(reset)
        self.assertIn("<tools>", delta)   # a reset re-sends the full render

    def test_omitted_and_explicit_none_tool_calls_are_treated_as_equivalent(self):
        # A client that never sends tool_calls at all, and a client that
        # sends tool_calls: None, must not be spuriously treated as having
        # diverged from each other -- both mean "no tool calls here."
        self.s.next_delta([U1], TOOLS)
        assistant_omitted = {"role": "assistant", "content": GEN}
        self.s.next_delta([U1, assistant_omitted, U2], TOOLS)
        assistant_none = {"role": "assistant", "content": GEN, "tool_calls": None}
        _, reset = self.s.next_delta([U1, assistant_none, U2], TOOLS)
        self.assertFalse(reset)

    def test_absent_none_and_empty_tool_calls_are_all_pairwise_equivalent(self):
        # GAP 1: all three spellings mean "no tool calls" to the template
        # and must compare equal to EACH OTHER, not just absent-vs-None.
        # Chained absent -> None -> [] -> absent covers all three adjacent
        # pairs; equivalence is symmetric so that closes the triangle.
        s = Session("gap1", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        assistant_absent = {"role": "assistant", "content": GEN}
        s.next_delta([U1, assistant_absent, U2], TOOLS)

        assistant_none = {"role": "assistant", "content": GEN, "tool_calls": None}
        _, reset_absent_to_none = s.next_delta([U1, assistant_none, U2], TOOLS)
        self.assertFalse(reset_absent_to_none)

        assistant_empty = {"role": "assistant", "content": GEN, "tool_calls": []}
        _, reset_none_to_empty = s.next_delta([U1, assistant_empty, U2], TOOLS)
        self.assertFalse(reset_none_to_empty)

        _, reset_empty_to_absent = s.next_delta([U1, assistant_absent, U2], TOOLS)
        self.assertFalse(reset_empty_to_absent)

    def test_a_regenerated_tool_call_id_does_not_force_a_reset(self):
        # GAP 2: the template (lines ~105-127) never renders tool_call.id,
        # so two calls differing ONLY in id are byte-identical to the
        # model. Many clients regenerate call ids per request; comparing
        # them would reset every turn for those clients.
        s = Session("gap2a", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        call_v1 = [{"id": "call_1", "type": "function",
                    "function": {"name": "terminal", "arguments": {"command": "ls"}}}]
        assistant_v1 = {"role": "assistant", "content": "", "tool_calls": call_v1}
        s.next_delta([U1, assistant_v1, U2], TOOLS)
        call_v2 = [{"id": "call_regenerated_9999", "type": "function",
                    "function": {"name": "terminal", "arguments": {"command": "ls"}}}]
        assistant_v2 = {"role": "assistant", "content": "", "tool_calls": call_v2}
        _, reset = s.next_delta([U1, assistant_v2, U2], TOOLS)
        self.assertFalse(reset)

    def test_tool_calls_differing_only_in_arguments_still_reset(self):
        # Proves GAP 2's id-exclusion did not loosen the check too far:
        # same id, same function name, DIFFERENT arguments must still
        # reset -- arguments are rendered by the template and are not
        # representational noise the way id is.
        s = Session("gap2b", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        call_ls = [{"id": "call_1", "type": "function",
                    "function": {"name": "terminal", "arguments": {"command": "ls"}}}]
        assistant_ls = {"role": "assistant", "content": "", "tool_calls": call_ls}
        s.next_delta([U1, assistant_ls, U2], TOOLS)
        call_rm = [{"id": "call_1", "type": "function",
                    "function": {"name": "terminal", "arguments": {"command": "rm -rf /"}}}]
        assistant_rm = {"role": "assistant", "content": "", "tool_calls": call_rm}
        delta, reset = s.next_delta([U1, assistant_rm, U2], TOOLS)
        self.assertTrue(reset)
        self.assertIn("<tools>", delta)

    def test_a_different_reasoning_content_with_matching_visible_content_still_resets(self):
        # GAP 3: the template DOES render reasoning_content (lines ~91-101)
        # when it is an explicit string. Two turns with identical VISIBLE
        # content but different reasoning_content render different bytes
        # and must reset -- the same bug class as the Critical already
        # fixed for tool_calls, one field over.
        s = Session("gap3", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        assistant_r1 = {"role": "assistant", "content": "same visible answer",
                        "reasoning_content": "reasoning A"}
        s.next_delta([U1, assistant_r1, U2], TOOLS)
        assistant_r2 = {"role": "assistant", "content": "same visible answer",
                        "reasoning_content": "reasoning B"}
        delta, reset = s.next_delta([U1, assistant_r2, U2], TOOLS)
        self.assertTrue(reset)
        self.assertIn("<tools>", delta)

    def test_render_initial_succeeds_with_json_string_arguments(self):
        # `arguments` in the real OpenAI wire format is a JSON-encoded
        # STRING (Task 2's parse_tool_calls emits `json.dumps(args)`), not
        # a mapping. The template renders `arguments|items`, which requires
        # a mapping -- render_initial (via next_delta's first turn) must
        # accept the string shape rather than raising TypeError.
        call = [{"id": "call_1", "type": "function",
                 "function": {"name": "terminal", "arguments": '{"command": "ls -la"}'}}]
        assistant = {"role": "assistant", "content": "", "tool_calls": call}
        delta, reset = self.s.next_delta([U1, assistant], TOOLS)  # must not raise
        self.assertFalse(reset)
        self.assertIn("command", delta)
        self.assertIn("ls -la", delta)

    def test_tool_calls_with_different_json_string_arguments_still_reset(self):
        # The under-comparison half of the same gap: two DIFFERENT
        # JSON-string payloads must not be silently treated as equal --
        # that would read a genuine rewrite as an append onto a stale KV.
        s = Session("json_diff", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        call_1 = [{"id": "call_1", "type": "function",
                   "function": {"name": "terminal", "arguments": '{"command": "ls"}'}}]
        assistant_1 = {"role": "assistant", "content": "", "tool_calls": call_1}
        s.next_delta([U1, assistant_1, U2], TOOLS)
        call_2 = [{"id": "call_1", "type": "function",
                   "function": {"name": "terminal", "arguments": '{"command": "rm -rf /"}'}}]
        assistant_2 = {"role": "assistant", "content": "", "tool_calls": call_2}
        delta, reset = s.next_delta([U1, assistant_2, U2], TOOLS)
        self.assertTrue(reset)
        self.assertIn("<tools>", delta)

    def test_json_string_and_dict_arguments_with_same_pairs_are_equivalent(self):
        # Both wire shapes for the SAME logical arguments must compare
        # equal -- a client echoing history back as a string must not
        # force a reset against a dict-shaped seed, or vice versa.
        s = Session("json_dict_equal", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        call_str = [{"id": "call_1", "type": "function",
                     "function": {"name": "terminal", "arguments": '{"command": "ls"}'}}]
        assistant_str = {"role": "assistant", "content": "", "tool_calls": call_str}
        s.next_delta([U1, assistant_str, U2], TOOLS)
        call_dict = [{"id": "call_1", "type": "function",
                      "function": {"name": "terminal", "arguments": {"command": "ls"}}}]
        assistant_dict = {"role": "assistant", "content": "", "tool_calls": call_dict}
        _, reset = s.next_delta([U1, assistant_dict, U2], TOOLS)
        self.assertFalse(reset)

    def test_malformed_json_arguments_do_not_crash_and_force_a_reset(self):
        # A string that fails to parse as JSON must not crash the render,
        # and must not be silently treated as "no arguments" (which would
        # risk two DIFFERENT malformed payloads comparing equal). The safe
        # branch is a reset.
        s = Session("malformed", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        call_ok = [{"id": "call_1", "type": "function",
                    "function": {"name": "terminal", "arguments": {"command": "ls"}}}]
        assistant_ok = {"role": "assistant", "content": "", "tool_calls": call_ok}
        s.next_delta([U1, assistant_ok, U2], TOOLS)
        call_broken = [{"id": "call_1", "type": "function",
                        "function": {"name": "terminal", "arguments": "{not valid json"}}]
        assistant_broken = {"role": "assistant", "content": "", "tool_calls": call_broken}
        delta, reset = s.next_delta([U1, assistant_broken, U2], TOOLS)  # must not raise
        self.assertTrue(reset)
        self.assertIn("<tools>", delta)

    def test_keep_reasoning_false_renders_tool_call_history_without_raising(self):
        # keep_reasoning=False takes the full render (render_initial) path
        # on EVERY turn by design, so this crash is reachable on turn 2
        # already, not just on a reset.
        s = Session("kr_false_tc", ChatTemplate.load(TPL), keep_reasoning=False)
        s.next_delta([U1], TOOLS)
        call = [{"id": "call_1", "type": "function",
                 "function": {"name": "terminal", "arguments": '{"command": "ls"}'}}]
        assistant = {"role": "assistant", "content": "", "tool_calls": call}
        delta, _ = s.next_delta([U1, assistant, U2], TOOLS)  # must not raise
        self.assertIn("<tools>", delta)


if __name__ == "__main__":
    unittest.main()
