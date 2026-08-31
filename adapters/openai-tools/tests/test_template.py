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
from openai_tools.template import ChatTemplate

TPL = "adapters/openai-tools/templates/qwen36-reap48-ours.jinja"
TOOLS = [{"type": "function", "function": {
    "name": "terminal", "description": "Run a shell command",
    "parameters": {"type": "object", "properties": {"command": {"type": "string"}}}}}]


class TemplateTest(unittest.TestCase):
    def setUp(self):
        self.t = ChatTemplate.load(TPL)

    def test_template_identity_is_pinned(self):
        self.assertTrue(self.t.sha256.startswith("e84f32a23fdda276"))

    def test_initial_render_carries_tools_and_ends_in_the_generation_prompt(self):
        out = self.t.render_initial(
            [{"role": "user", "content": "List files in /tmp"}], TOOLS)
        self.assertIn("<tools>", out)
        self.assertIn('"name": "terminal"', out.replace('"name":"terminal"', '"name": "terminal"'))
        self.assertTrue(out.endswith("<|im_start|>assistant\n<think>\n"))

    def test_render_turns_is_derived_from_the_template_not_hand_written(self):
        # The derived per-turn bytes must equal what the template itself
        # produces when those turns are appended to a conversation.
        base = [{"role": "user", "content": "A"}]
        added = [{"role": "assistant", "content": "R"},
                 {"role": "user", "content": "B"}]
        derived = self.t.render_turns(added)
        whole = self.t._render(base + added, TOOLS, generation=False)
        prefix = self.t._render(base, TOOLS, generation=False)
        self.assertTrue(whole.startswith(prefix))
        self.assertEqual(derived, whole[len(prefix):] )

    def test_every_turn_rendering_starts_at_a_special_token_boundary(self):
        out = self.t.render_turns([{"role": "user", "content": "B"}])
        self.assertTrue(out.startswith("<|im_start|>"))


if __name__ == "__main__":
    unittest.main()
