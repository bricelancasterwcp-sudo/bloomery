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

"""Rendering through the model's own chat template.

The template is authoritative. The per-turn wrapper is *derived* from it by
differential rendering rather than hand-written, so a template change is
caught by a test instead of silently producing prompts the model was never
trained on.
"""
import hashlib

from jinja2 import Environment
from jinja2.exceptions import TemplateError

GENERATION_SUFFIX = "<|im_start|>assistant\n<think>\n"


def _raise_exception(message):
    # Qwen templates call this; jinja2 does not provide it.
    raise TemplateError(message)


class ChatTemplate:
    def __init__(self, source: str):
        self.source = source
        self.sha256 = hashlib.sha256(source.encode("utf-8")).hexdigest()
        env = Environment()
        env.globals["raise_exception"] = _raise_exception
        self._tpl = env.from_string(source)
        self.GENERATION_SUFFIX = GENERATION_SUFFIX

    @classmethod
    def load(cls, path: str) -> "ChatTemplate":
        with open(path, encoding="utf-8") as handle:
            return cls(handle.read())

    def _render(self, messages, tools, generation: bool) -> str:
        return self._tpl.render(
            messages=messages, tools=tools, add_generation_prompt=generation
        )

    def render_initial(self, messages, tools) -> str:
        """The first send of a session: system block, tool schemas, the
        opening turns, and the generation prompt."""
        return self._render(messages, tools, generation=True)

    def render_turns(self, messages) -> str:
        """The bytes the template produces for `messages` appended to an
        existing conversation — derived, never hand-written ChatML.

        A sentinel base turn is rendered with and without `messages`; the
        difference is exactly the appended turns. Tools are omitted from
        both sides so the invariant preamble cancels out.
        """
        base = [{"role": "user", "content": "\x00sentinel\x00"}]
        prefix = self._render(base, None, generation=False)
        whole = self._render(base + list(messages), None, generation=False)
        if not whole.startswith(prefix):
            raise TemplateError(
                "template does not append cleanly: the per-turn wrapper cannot "
                "be derived, so history would be rendered as bytes the model "
                "never saw in training"
            )
        return whole[len(prefix):]
