import json
import unittest

from openai_tools.toolcall import parse_tool_calls, split_reasoning

TOOLS = [{"type": "function", "function": {
    "name": "terminal",
    "parameters": {"type": "object", "properties": {
        "command": {"type": "string"},
        "timeout": {"type": "integer"},
        "quiet": {"type": "boolean"}}}}}]

CALL = ("<tool_call>\n<function=terminal>\n"
        "<parameter=command>\nls /tmp\n</parameter>\n"
        "</function>\n</tool_call>")


class SplitReasoningTest(unittest.TestCase):
    def test_reasoning_is_separated_from_visible_output(self):
        reasoning, visible = split_reasoning("thinking hard\n</think>\n\nhello")
        self.assertEqual(reasoning, "thinking hard")
        self.assertEqual(visible, "hello")

    def test_output_without_a_close_tag_is_all_visible(self):
        reasoning, visible = split_reasoning("just an answer")
        self.assertEqual(reasoning, "")
        self.assertEqual(visible, "just an answer")


class ParseToolCallsTest(unittest.TestCase):
    def test_a_well_formed_call_becomes_an_openai_tool_call(self):
        calls = parse_tool_calls(CALL, TOOLS)
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0]["type"], "function")
        self.assertEqual(calls[0]["function"]["name"], "terminal")
        self.assertEqual(json.loads(calls[0]["function"]["arguments"]),
                         {"command": "ls /tmp"})
        self.assertTrue(calls[0]["id"])

    def test_parameters_are_coerced_to_their_declared_schema_types(self):
        raw = ("<tool_call>\n<function=terminal>\n"
               "<parameter=command>\nls\n</parameter>\n"
               "<parameter=timeout>\n30\n</parameter>\n"
               "<parameter=quiet>\ntrue\n</parameter>\n"
               "</function>\n</tool_call>")
        args = json.loads(parse_tool_calls(raw, TOOLS)[0]["function"]["arguments"])
        self.assertEqual(args["timeout"], 30)
        self.assertIs(args["quiet"], True)

    def test_two_calls_in_one_turn_both_parse(self):
        calls = parse_tool_calls(CALL + "\n" + CALL, TOOLS)
        self.assertEqual(len(calls), 2)
        self.assertNotEqual(calls[0]["id"], calls[1]["id"])

    def test_prose_with_no_call_returns_none(self):
        self.assertIsNone(parse_tool_calls("I cannot help with that.", TOOLS))

    def test_a_truncated_call_returns_none_rather_than_a_guess(self):
        self.assertIsNone(parse_tool_calls(
            "<tool_call>\n<function=terminal>\n<parameter=command>\nls", TOOLS))

    def test_an_unknown_function_name_returns_none(self):
        raw = CALL.replace("terminal", "rm_rf")
        self.assertIsNone(parse_tool_calls(raw, TOOLS))

    def test_a_value_that_will_not_coerce_returns_none_not_a_wrong_type(self):
        raw = ("<tool_call>\n<function=terminal>\n"
               "<parameter=timeout>\nnot-a-number\n</parameter>\n"
               "</function>\n</tool_call>")
        self.assertIsNone(parse_tool_calls(raw, TOOLS))

    def test_multiple_functions_in_one_call_block_returns_none(self):
        """Finding 1: Multiple functions in one block silently drops all but the first."""
        raw = ("<tool_call>\n<function=terminal>\n"
               "<parameter=command>\nls\n</parameter>\n"
               "</function>\n<function=terminal>\n"
               "<parameter=command>\nrm -rf /\n</parameter>\n"
               "</function>\n</tool_call>")
        self.assertIsNone(parse_tool_calls(raw, TOOLS))

    def test_a_parameter_value_containing_close_tag_returns_none(self):
        """Finding 2: Non-greedy regex silently truncates values containing </parameter>."""
        raw = ("<tool_call>\n<function=terminal>\n"
               "<parameter=command>\necho </parameter> injected\n</parameter>\n"
               "</function>\n</tool_call>")
        self.assertIsNone(parse_tool_calls(raw, TOOLS))

    def test_an_undeclared_parameter_returns_none(self):
        """Finding 3: Undeclared parameters are silently accepted as strings."""
        raw = ("<tool_call>\n<function=terminal>\n"
               "<parameter=extra_evil>\nsomething\n</parameter>\n"
               "</function>\n</tool_call>")
        self.assertIsNone(parse_tool_calls(raw, TOOLS))

    def test_balanced_fake_parameter_tags_in_value_returns_none(self):
        """Finding 2 (refined): Losslessness check.

        A value containing <parameter= or </parameter> is ambiguous and
        must be refused. This bypass has balanced tag counts but still
        silently truncates the SECRET_TAIL.
        """
        raw = ("<tool_call>\n<function=terminal>\n"
               "<parameter=command>\nsome text <parameter=x>content</parameter> more text SECRET_TAIL\n</parameter>\n"
               "</function>\n</tool_call>")
        self.assertIsNone(parse_tool_calls(raw, TOOLS))

    def test_legitimate_two_parameter_call_still_parses(self):
        """Verify we have not over-refused: two declared parameters must work."""
        raw = ("<tool_call>\n<function=terminal>\n"
               "<parameter=command>\nls\n</parameter>\n"
               "<parameter=timeout>\n30\n</parameter>\n"
               "</function>\n</tool_call>")
        calls = parse_tool_calls(raw, TOOLS)
        self.assertIsNotNone(calls)
        args = json.loads(calls[0]["function"]["arguments"])
        self.assertEqual(args["command"], "ls")
        self.assertEqual(args["timeout"], 30)

    def test_value_containing_word_parameter_still_parses(self):
        """Verify we reject only the delimiters, not the word 'parameter' in ordinary text."""
        raw = ("<tool_call>\n<function=terminal>\n"
               "<parameter=command>\nshow parameters\n</parameter>\n"
               "</function>\n</tool_call>")
        calls = parse_tool_calls(raw, TOOLS)
        self.assertIsNotNone(calls)
        args = json.loads(calls[0]["function"]["arguments"])
        self.assertEqual(args["command"], "show parameters")

    def test_junk_before_function_in_block_returns_none(self):
        """Finding 2 (block level): Junk before function tag in block is silently dropped."""
        raw = ("<tool_call>\nPRECEDING_JUNK<function=terminal>\n"
               "<parameter=command>\nls\n</parameter>\n"
               "</function>\n</tool_call>")
        self.assertIsNone(parse_tool_calls(raw, TOOLS))

    def test_junk_after_function_in_block_returns_none(self):
        """Finding 2 (block level): Junk after function tag in block is silently dropped."""
        raw = ("<tool_call>\n<function=terminal>\n"
               "<parameter=command>\nls\n</parameter>\n"
               "</function>\nSECRET_TAIL_NO_FAKE_TAG\n</tool_call>")
        self.assertIsNone(parse_tool_calls(raw, TOOLS))

    def test_fake_incomplete_tag_in_block_returns_none(self):
        """Finding 2 (block level): Incomplete fake tag in block is silently dropped."""
        raw = ("<tool_call>\n<function=terminal>\n"
               "<parameter=command>\nls\n</parameter>\n"
               "</function>\nSECRET<function=incomplete>\n</tool_call>")
        self.assertIsNone(parse_tool_calls(raw, TOOLS))

    def test_call_with_preceding_prose_still_parses(self):
        """Verify we do not over-refuse: prose before <tool_call> is legitimate."""
        raw = "I will list the files.\n<tool_call>\n<function=terminal>\n<parameter=command>\nls\n</parameter>\n</function>\n</tool_call>"
        calls = parse_tool_calls(raw, TOOLS)
        self.assertIsNotNone(calls)
        args = json.loads(calls[0]["function"]["arguments"])
        self.assertEqual(args["command"], "ls")

    def test_call_with_surrounding_whitespace_still_parses(self):
        """Verify we do not over-refuse: whitespace around <tool_call> blocks is legitimate."""
        raw = "   \n  <tool_call>\n<function=terminal>\n<parameter=command>\nls\n</parameter>\n</function>\n</tool_call>  \n  "
        calls = parse_tool_calls(raw, TOOLS)
        self.assertIsNotNone(calls)
        args = json.loads(calls[0]["function"]["arguments"])
        self.assertEqual(args["command"], "ls")

    def test_duplicate_parameter_key_returns_none(self):
        """Finding A: a repeated parameter key silently overwrites the earlier value
        (`args[key] = coerced`), discarding it even though byte-span coverage of the
        function body is complete. Semantic coverage, not just byte coverage, matters."""
        raw = ("<tool_call><function=terminal>"
               "<parameter=command>rm -rf /important</parameter>"
               "<parameter=command>ls</parameter>"
               "</function></tool_call>")
        self.assertIsNone(parse_tool_calls(raw, TOOLS))

    def test_unterminated_tool_call_after_a_valid_one_returns_none(self):
        """Finding B: `_CALL.findall` requires a closing </tool_call>, so a second,
        unterminated opener (e.g. from a generation cut off by max_tokens) is never
        examined and its content silently vanishes."""
        raw = (CALL + "\n"
               "<tool_call><function=terminal>"
               "<parameter=command>rm -rf /DROPPED</parameter>"
               "</function>")
        self.assertIsNone(parse_tool_calls(raw, TOOLS))


if __name__ == "__main__":
    unittest.main()
