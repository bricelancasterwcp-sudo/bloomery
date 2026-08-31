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

from openai_tools.session import Session, UnrenderableMessage
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

    def test_a_whitespace_only_difference_in_tool_call_arguments_is_still_an_append(self):
        # Live finding (2026-08-31, measured against hermes): the adapter
        # serialises tool_calls[].function.arguments via json.dumps(args)
        # (toolcall.py's parse_tool_calls), which uses Python's default
        # separators -- a space after both `:` and `,`. hermes echoes the
        # same call back re-serialised COMPACTLY (no spaces). Both strings
        # parse to the identical dict, in the identical key order (json.loads
        # is whitespace-insensitive and preserves the order keys appear in
        # the source text), so this must compare equal and stay an append --
        # not force a reset and a needless full <tools> re-render.
        s = Session("whitespace", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        spaced = '{"content": "world", "path": "/tmp/x.txt"}'
        generated = {"role": "assistant", "content": None, "tool_calls": [
            {"id": "call_1", "type": "function",
             "function": {"name": "write_file", "arguments": spaced}}]}
        s.record_generation(generated)
        compact = '{"content":"world","path":"/tmp/x.txt"}'
        echoed = {"role": "assistant", "content": "", "tool_calls": [
            {"id": "call_1", "type": "function",
             "function": {"name": "write_file", "arguments": compact}}]}
        tool_result = {"role": "tool", "tool_call_id": "call_1", "content": "ok"}
        delta, reset = s.next_delta([U1, echoed, tool_result], TOOLS)
        self.assertFalse(reset)
        self.assertNotIn("<tools>", delta)

    def test_tool_call_arguments_with_a_different_key_order_is_still_an_append(self):
        # REVERSED 2026-08-31 (was
        # test_tool_call_arguments_with_a_different_key_order_still_resets):
        # live-captured against a real hermes trajectory, the adapter emits
        # tool_calls[].function.arguments in the order the model produced
        # the parameters; the client echoes the SAME call back with its
        # keys reordered. Order-sensitivity here was justified by "the
        # template renders arguments|items in dict order, so reordering
        # changes the rendered bytes" -- true, but irrelevant: the append
        # path never re-renders history at all (the KV already holds the
        # bytes this adapter itself produced; only the NEW turns are sent),
        # and the reset path re-renders everything from the client's
        # CURRENT messages self-consistently regardless of order. So key
        # order in a client's echo can never actually change what the model
        # sees, and comparing it order-sensitively bought no correctness
        # while forcing a reset (and a full <tools> re-render, ~6.2k
        # tokens) on every single turn of a real client trajectory. This is
        # an authorised expectation change, not a weakened test: the SAME
        # key/value pairs in a DIFFERENT key order must now be an append.
        s = Session("arg_order", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        call_1 = [{"id": "call_1", "type": "function",
                   "function": {"name": "write_file",
                                 "arguments": '{"content": "world", "path": "/tmp/x.txt"}'}}]
        assistant_1 = {"role": "assistant", "content": "", "tool_calls": call_1}
        s.next_delta([U1, assistant_1, U2], TOOLS)
        call_2 = [{"id": "call_1", "type": "function",
                   "function": {"name": "write_file",
                                 "arguments": '{"path": "/tmp/x.txt", "content": "world"}'}}]
        assistant_2 = {"role": "assistant", "content": "", "tool_calls": call_2}
        delta, reset = s.next_delta([U1, assistant_2, U2], TOOLS)
        self.assertFalse(reset)
        self.assertNotIn("<tools>", delta)

    def test_a_real_hermes_trajectory_key_reorder_is_an_append_not_a_reset(self):
        # THE reproduction: captured live against a real hermes
        # trajectory. The adapter emits arguments in the order the model
        # produced them; hermes echoes the same call back with the keys
        # reordered. This must be an APPEND (was_reset False) and must NOT
        # re-render the <tools> preamble.
        s = Session("hermes_live", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        emitted = [{"id": "call_1", "type": "function",
                    "function": {"name": "write_file",
                                 "arguments": '{"path": "/tmp/x.txt", "content": "hello"}'}}]
        generated = {"role": "assistant", "content": None, "tool_calls": emitted}
        s.record_generation(generated)
        echoed = [{"id": "call_1", "type": "function",
                   "function": {"name": "write_file",
                                "arguments": '{"content":"hello","path":"/tmp/x.txt"}'}}]
        echoed_message = {"role": "assistant", "content": "", "tool_calls": echoed}
        tool_result = {"role": "tool", "tool_call_id": "call_1", "content": "ok"}
        delta, reset = s.next_delta([U1, echoed_message, tool_result], TOOLS)
        self.assertFalse(reset)
        self.assertNotIn("<tools>", delta)

    def test_tool_call_arguments_with_a_different_value_still_resets(self):
        # Second over-normalisation guard: same key, DIFFERENT value, via
        # the JSON-string shape the whitespace fix touches.
        s = Session("arg_value", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        call_1 = [{"id": "call_1", "type": "function",
                   "function": {"name": "write_file",
                                 "arguments": '{"content": "world", "path": "/tmp/x.txt"}'}}]
        assistant_1 = {"role": "assistant", "content": "", "tool_calls": call_1}
        s.next_delta([U1, assistant_1, U2], TOOLS)
        call_2 = [{"id": "call_1", "type": "function",
                   "function": {"name": "write_file",
                                 "arguments": '{"content": "moon", "path": "/tmp/x.txt"}'}}]
        assistant_2 = {"role": "assistant", "content": "", "tool_calls": call_2}
        delta, reset = s.next_delta([U1, assistant_2, U2], TOOLS)
        self.assertTrue(reset)
        self.assertIn("<tools>", delta)

    def test_tool_call_arguments_with_a_different_key_set_still_resets(self):
        # Third over-normalisation guard: same size, DIFFERENT key set.
        s = Session("arg_keyset", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        call_1 = [{"id": "call_1", "type": "function",
                   "function": {"name": "write_file",
                                 "arguments": '{"content": "world", "path": "/tmp/x.txt"}'}}]
        assistant_1 = {"role": "assistant", "content": "", "tool_calls": call_1}
        s.next_delta([U1, assistant_1, U2], TOOLS)
        call_2 = [{"id": "call_1", "type": "function",
                   "function": {"name": "write_file",
                                 "arguments": '{"content": "world", "mode": "w"}'}}]
        assistant_2 = {"role": "assistant", "content": "", "tool_calls": call_2}
        delta, reset = s.next_delta([U1, assistant_2, U2], TOOLS)
        self.assertTrue(reset)
        self.assertIn("<tools>", delta)

    def test_tool_call_identity_never_equates_two_malformed_arguments_strings(self):
        # Defense in depth for the whitespace fix's own parsing: even if
        # _tool_call_identity were ever reached with a malformed `arguments`
        # string (in the actual request path this is moot --
        # _normalize_tool_call already refuses it with UnrenderableMessage
        # before any identity comparison happens, see the malformed-JSON
        # tests below), two separately-computed identities for the SAME
        # malformed text must never compare equal to each other. Silently
        # treating a malformed call as "the same as before" is exactly the
        # unsafe shortcut this fix must not introduce.
        broken = [{"id": "call_1", "type": "function",
                   "function": {"name": "terminal", "arguments": "{not valid json"}}]
        identity_1 = Session._tool_call_identity(broken)
        identity_2 = Session._tool_call_identity(broken)
        self.assertNotEqual(identity_1, identity_2)

    def test_malformed_json_arguments_raises_and_names_the_tool_call(self):
        # Round 4 supersedes Round 3's "blank it and reset" behaviour: the
        # coordinator overruled that as a silent misrepresentation (the
        # blanked <function=...></function> with zero parameters is the
        # exact byte string that would be sent to the model, showing it a
        # tool call that did not happen). A malformed `arguments` string is
        # a CLIENT error under the OpenAI wire format; the honest response
        # is a typed, controlled refusal, not an invisible repair.
        s = Session("malformed", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        call_broken = [{"id": "call_1", "type": "function",
                        "function": {"name": "terminal", "arguments": "{not valid json"}}]
        assistant_broken = {"role": "assistant", "content": "", "tool_calls": call_broken}
        with self.assertRaises(UnrenderableMessage) as ctx:
            s.next_delta([U1, assistant_broken, U2], TOOLS)
        self.assertEqual(ctx.exception.function_name, "terminal")

    def test_the_raw_malformed_string_never_appears_in_the_exception_message(self):
        # The malformed payload is untrusted client data; it must not be
        # forced into logs via this exception's own message.
        s = Session("malformed_secret", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        secret_marker = "TOTALLY_NOT_VALID_JSON_SENTINEL_XYZ"
        call_broken = [{"id": "call_1", "type": "function",
                        "function": {"name": "terminal", "arguments": secret_marker}}]
        assistant_broken = {"role": "assistant", "content": "", "tool_calls": call_broken}
        with self.assertRaises(UnrenderableMessage) as ctx:
            s.next_delta([U1, assistant_broken, U2], TOOLS)
        self.assertNotIn(secret_marker, str(ctx.exception))

    def test_a_raise_leaves_session_state_unchanged_for_a_retry(self):
        # A caller that fixes its request and retries must not be stuck
        # with half-mutated tracking: session state after the raise must
        # be exactly what it was before the failed call was attempted.
        s = Session("malformed_retry", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        call_broken = [{"id": "call_1", "type": "function",
                        "function": {"name": "terminal", "arguments": "{not valid json"}}]
        assistant_broken = {"role": "assistant", "content": "", "tool_calls": call_broken}
        with self.assertRaises(UnrenderableMessage):
            s.next_delta([U1, assistant_broken, U2], TOOLS)

        # A subsequent well-formed call must behave exactly as if the bad
        # call had never been attempted: compare against a fresh session
        # that only ever saw the same successful first turn.
        fresh = Session("fresh_control", ChatTemplate.load(TPL))
        fresh.next_delta([U1], TOOLS)
        expected_delta, expected_reset = fresh.next_delta(
            [U1, {"role": "assistant", "content": GEN}, U2], TOOLS)

        actual_delta, actual_reset = s.next_delta(
            [U1, {"role": "assistant", "content": GEN}, U2], TOOLS)
        self.assertEqual(actual_delta, expected_delta)
        self.assertEqual(actual_reset, expected_reset)

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

    def test_record_generation_also_accepts_a_full_assistant_message_dict(self):
        # Task 5 authorised extension: a content-only string can never
        # carry tool_calls, so a caller returning {content, tool_calls}
        # to its own client has no way to make what is tracked here
        # match a later real echo of that object -- only the dict form
        # can. The dict is normalized the same way next_delta normalizes
        # incoming history, so a real client's echo (arguments as a JSON
        # string, the actual OpenAI wire shape) compares correctly.
        s = Session("dict_record", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        generated = {"role": "assistant", "content": None,
                     "tool_calls": [{"id": "call_1", "type": "function",
                                     "function": {"name": "terminal",
                                                  "arguments": '{"command": "ls"}'}}]}
        s.record_generation(generated)
        # A real client echoes the exact object back (arguments still a
        # JSON string, since that is what was handed out over the wire).
        delta, reset = s.next_delta([U1, generated, U2], TOOLS)
        self.assertFalse(reset)
        self.assertNotIn("<tools>", delta)

    def test_a_changed_tool_set_forces_a_reset(self):
        # Important 3: the append path never re-renders `tools` at all
        # (`render_turns` takes no tools argument), so if the client's tool
        # set differs from what turn 1 baked into the resident KV, an
        # append would leave the model's context stale while parsing
        # against the NEW schema -- the same silent-corruption class
        # `_is_extension` exists to catch for `messages`.
        other_tools = [{"type": "function", "function": {
            "name": "other_tool", "parameters": {"type": "object", "properties": {}}}}]
        s = Session("tools_diverge", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        assistant = {"role": "assistant", "content": GEN}
        delta, reset = s.next_delta([U1, assistant, U2], other_tools)
        self.assertTrue(reset)
        self.assertIn("<tools>", delta)   # a reset re-sends the full render

    def test_an_unchanged_tool_set_is_not_treated_as_divergence(self):
        # The under-comparison half of the same gap: the SAME tools value
        # (by content, not identity) must not spuriously force a reset.
        same_tools_new_list = list(TOOLS)
        s = Session("tools_same", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        assistant = {"role": "assistant", "content": GEN}
        _, reset = s.next_delta([U1, assistant, U2], same_tools_new_list)
        self.assertFalse(reset)

    def test_snapshot_and_restore_undo_a_next_delta_call(self):
        # Critical 2's mechanism: a caller takes a snapshot BEFORE calling
        # next_delta; if the resulting bytes are never actually delivered
        # downstream (the daemon call raised), restoring that snapshot
        # undoes next_delta's mutation, so an identical retry reproduces
        # the identical delta -- not a stunted one built on the false
        # assumption that the failed send had succeeded.
        s = Session("snap", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        s.record_generation(GEN)

        turn2_messages = [U1, {"role": "assistant", "content": GEN}, U2]
        snap = s.snapshot()
        first_attempt, first_reset = s.next_delta(turn2_messages, TOOLS)
        s.restore(snap)  # the downstream send of first_attempt never landed

        retry, retry_reset = s.next_delta(turn2_messages, TOOLS)
        self.assertEqual(retry, first_attempt)
        self.assertEqual(retry_reset, first_reset)

    def test_record_generation_string_form_is_unchanged(self):
        # The original contract: a plain string is still treated as a
        # content-only assistant turn, exactly as before this extension.
        s = Session("string_record", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        s.record_generation(GEN)
        assistant = {"role": "assistant", "content": GEN}
        delta, reset = s.next_delta([U1, assistant, U2], TOOLS)
        self.assertFalse(reset)
        self.assertNotIn("<tools>", delta)

    def test_none_content_echoed_back_as_empty_string_is_still_an_append(self):
        # Live finding (2026-08-31, measured against hermes): the adapter
        # returns content: None on a tool-call turn (correct per the
        # OpenAI shape, since content and tool_calls are mutually
        # exclusive on the wire). hermes echoes that same turn back with
        # content: "". Both spellings mean "no text" -- exactly the case
        # for a tool-call turn -- so they must compare equal. Before this
        # fix, `None != ""` made _is_extension see every tool-call turn as
        # changed, forcing a reset (and a full <tools> re-render) on every
        # single turn -- silently defeating the prefill-once property that
        # is this adapter's entire economic justification.
        s = Session("none_vs_empty", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        generated = {"role": "assistant", "content": None, "tool_calls": TC_A}
        s.record_generation(generated)
        echoed = {"role": "assistant", "content": "", "tool_calls": TC_A}
        tool_result = {"role": "tool", "tool_call_id": "call_1", "content": "ok"}
        delta, reset = s.next_delta([U1, echoed, tool_result], TOOLS)
        self.assertFalse(reset)
        self.assertNotIn("<tools>", delta)

    def test_empty_string_content_echoed_back_as_none_is_still_an_append(self):
        # The mirror direction of the same equivalence: an adapter-side ""
        # echoed back by a client as None must also be an append, not a
        # reset -- the normalization is symmetric.
        s = Session("empty_vs_none", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        generated = {"role": "assistant", "content": "", "tool_calls": TC_A}
        s.record_generation(generated)
        echoed = {"role": "assistant", "content": None, "tool_calls": TC_A}
        tool_result = {"role": "tool", "tool_call_id": "call_1", "content": "ok"}
        delta, reset = s.next_delta([U1, echoed, tool_result], TOOLS)
        self.assertFalse(reset)
        self.assertNotIn("<tools>", delta)

    def test_content_x_vs_empty_string_still_resets(self):
        # Guard against over-normalising: the equivalence is only between
        # the two EMPTY spellings (None and ""). A turn with actual text
        # is not "no text" under either spelling and must still reset.
        s = Session("x_vs_empty", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        assistant_x = {"role": "assistant", "content": "x"}
        s.next_delta([U1, assistant_x, U2], TOOLS)
        assistant_empty = {"role": "assistant", "content": ""}
        delta, reset = s.next_delta([U1, assistant_empty, U2], TOOLS)
        self.assertTrue(reset)
        self.assertIn("<tools>", delta)

    def test_content_x_vs_none_still_resets(self):
        # Same guard, the other empty spelling.
        s = Session("x_vs_none", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        assistant_x = {"role": "assistant", "content": "x"}
        s.next_delta([U1, assistant_x, U2], TOOLS)
        assistant_none = {"role": "assistant", "content": None}
        delta, reset = s.next_delta([U1, assistant_none, U2], TOOLS)
        self.assertTrue(reset)
        self.assertIn("<tools>", delta)


class RetryStateTest(unittest.TestCase):
    """Spec's 2026-08-31 amendment "the retry state" (from the live
    acceptance run): `Session` tracks the CLIENT view -- the message list
    as the client last actually sent it, BEFORE this session appended the
    assistant turn it generated for that turn -- separately from the KV
    view (`_sent_messages`, which DOES include that appended turn).
    `is_retry` reports whether an incoming list equals the client view AND
    the incoming tool set matches what is resident (Fix round 1, Finding
    1: reusing `_sent_tools` -- the same store `next_delta`'s own
    tools-divergence check already uses -- rather than a second notion of
    tool identity), reusing `_is_extension`'s own per-message comparison
    for the messages half (by calling it on two equal-length lists, which
    degenerates the zip-and-compare loop into strict equality) rather
    than a second, looser comparison that could disagree with it.

    This is exactly the gap the live acceptance run hit: after a
    successful turn, `record_generation` appends the assistant turn to
    the KV view, so a byte-identical client retry -- shorter than the KV
    view -- was misclassified as a rewrite. The client view never gained
    that appended turn, so the retry equals it exactly.
    """

    def setUp(self):
        self.s = Session("retry1", ChatTemplate.load(TPL))

    def test_is_retry_is_false_before_any_turn_has_ever_completed(self):
        self.assertFalse(self.s.is_retry([U1], TOOLS))

    def test_is_retry_is_true_for_the_exact_request_the_client_sent(self):
        # After turn 1, record_generation appends the assistant turn to
        # _sent_messages (the KV view) -- but the CLIENT never sent that
        # turn. The client view is still just [U1].
        self.s.next_delta([U1], TOOLS)
        self.s.record_generation(GEN)
        self.assertTrue(self.s.is_retry([U1], TOOLS))

    def test_is_retry_is_false_for_a_genuine_follow_up(self):
        self.s.next_delta([U1], TOOLS)
        self.s.record_generation(GEN)
        assistant = {"role": "assistant", "content": GEN}
        self.assertFalse(self.s.is_retry([U1, assistant, U2], TOOLS))

    def test_is_retry_is_false_for_a_genuine_rewrite(self):
        self.s.next_delta([U1], TOOLS)
        self.s.record_generation(GEN)
        self.assertFalse(self.s.is_retry([U2], TOOLS))  # compressed history, not a retry

    def test_is_retry_recognises_a_retry_after_a_multi_turn_conversation(self):
        # The bug this fixes was not turn-1-specific; neither is the fix.
        self.s.next_delta([U1], TOOLS)
        self.s.record_generation(GEN)
        assistant = {"role": "assistant", "content": GEN}
        second_request = [U1, assistant, U2]
        self.s.next_delta(second_request, TOOLS)
        self.s.record_generation(GEN)
        # Retry of turn 2's request, not turn 1's.
        self.assertTrue(self.s.is_retry(second_request, TOOLS))
        self.assertFalse(self.s.is_retry([U1], TOOLS))

    def test_is_retry_reuses_is_extensions_tool_call_identity_so_they_cannot_disagree(self):
        # A regenerated tool_call id is representational noise the template
        # never renders (Gap 2) -- _is_extension already ignores it, and
        # since is_retry reuses that exact comparison rather than a second
        # one, a retry whose call id was regenerated by the client must
        # still be recognised as a retry.
        s = Session("retry_tc", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        call_v1 = [{"id": "call_1", "type": "function",
                    "function": {"name": "terminal", "arguments": {"command": "ls"}}}]
        assistant_v1 = {"role": "assistant", "content": "", "tool_calls": call_v1}
        s.next_delta([U1, assistant_v1, U2], TOOLS)
        s.record_generation(GEN)
        call_v2 = [{"id": "call_regenerated_9999", "type": "function",
                    "function": {"name": "terminal", "arguments": {"command": "ls"}}}]
        retried_with_regenerated_id = [U1, {"role": "assistant", "content": "",
                                            "tool_calls": call_v2}, U2]
        self.assertTrue(s.is_retry(retried_with_regenerated_id, TOOLS))

    def test_client_view_is_restored_alongside_the_kv_view_on_a_failed_send(self):
        # Critical 2's mechanism must cover the client view too: if the
        # downstream send of next_delta's bytes never actually lands,
        # restoring the snapshot must undo the client-view mutation as
        # well as the KV-view one, or a retry check right after a failed
        # attempt would compare against the wrong prior state.
        self.s.next_delta([U1], TOOLS)
        self.s.record_generation(GEN)
        assistant = {"role": "assistant", "content": GEN}
        snap = self.s.snapshot()
        self.s.next_delta([U1, assistant, U2], TOOLS)  # not yet delivered downstream
        self.s.restore(snap)
        self.assertTrue(self.s.is_retry([U1], TOOLS))
        self.assertFalse(self.s.is_retry([U1, assistant, U2], TOOLS))

    def test_is_retry_is_false_when_the_tool_set_changed(self):
        # Fix round 1, Finding 1 (live-verified): a changed tool set means
        # the resident <tools> block is stale -- the divergence rule must
        # outrank the retry rule, not the other way round. Reuses
        # _sent_tools (the same store next_delta's tools_diverged check
        # already uses), not a second notion of tool identity.
        self.s.next_delta([U1], TOOLS)
        self.s.record_generation(GEN)
        other_tools = [{"type": "function", "function": {
            "name": "other_tool", "parameters": {"type": "object", "properties": {}}}}]
        self.assertFalse(self.s.is_retry([U1], other_tools))

    def test_is_retry_is_true_when_the_tool_set_is_unchanged(self):
        # The under-comparison half of the same fix: an equal (by content,
        # not identity) tools value must not spuriously defeat a retry.
        self.s.next_delta([U1], TOOLS)
        self.s.record_generation(GEN)
        same_tools_new_list = list(TOOLS)
        self.assertTrue(self.s.is_retry([U1], same_tools_new_list))

    def test_is_retry_recognises_a_retry_differing_only_by_none_vs_empty_content(self):
        # is_retry reuses _is_extension's own per-message comparison rather
        # than a second one (see the class docstring) -- so the None/""
        # content normalization must be inherited automatically, not
        # re-implemented. A retry whose only difference from the client
        # view is None vs "" content on the assistant turn must still be
        # recognised as a retry.
        s = Session("retry_none_vs_empty", ChatTemplate.load(TPL))
        s.next_delta([U1], TOOLS)
        s.record_generation({"role": "assistant", "content": None, "tool_calls": TC_A})
        tool_result = {"role": "tool", "tool_call_id": "call_1", "content": "ok"}
        turn2_request = [U1, {"role": "assistant", "content": None, "tool_calls": TC_A},
                          tool_result]
        s.next_delta(turn2_request, TOOLS)
        s.record_generation(GEN)
        retried_with_empty_content = [U1, {"role": "assistant", "content": "",
                                            "tool_calls": TC_A}, tool_result]
        self.assertTrue(s.is_retry(retried_with_empty_content, TOOLS))


if __name__ == "__main__":
    unittest.main()
