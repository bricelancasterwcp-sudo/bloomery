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

"""One client session, one bloomery agent, and the bytes its KV holds.

bloomery APPENDS: `generate_from` starts at `kv_cache_seq_pos_max + 1` with
`AddBos::Never`. Re-sending a conversation would duplicate it into the cache,
so this class exists to send only what is new — and, per the spec's
2026-08-31 amendment, to do that WITHOUT re-rendering history, because the
template rewrites historical assistant turns and a re-render is therefore not
a prefix extension of what was actually sent.
"""
import json


class Session:
    def __init__(self, agent_id: str, template, keep_reasoning: bool = True):
        self.agent_id = agent_id
        self.template = template
        self.keep_reasoning = keep_reasoning
        self._sent_messages: list[dict] = []
        self._open_assistant_turn = False

    @staticmethod
    def _normalize_tool_call(call):
        """Coerce one tool call's `arguments` to the mapping the template
        requires (it renders `arguments|items`), accepting BOTH wire
        shapes: an already-parsed dict, and the JSON-encoded STRING that
        Task 2's `parse_tool_calls` actually emits (`json.dumps(args)`) --
        the shape a client echoing history back will therefore send. The
        real OpenAI wire format is the string shape, so this is not an
        edge case; it is the common one.

        Returns `(normalized_call, unparseable)`. `unparseable` is True
        iff `arguments` was a string that failed to parse as a JSON
        object. That failure is NEVER treated as "no arguments" -- doing
        so would silently discard a real, if malformed, payload and risk
        two DIFFERENT malformed payloads comparing equal (a false append
        onto a stale KV, the same catastrophic direction as the tool_calls
        Critical already fixed). It is also never handed to the template
        as a raw string, since `arguments|items` raises TypeError on
        anything but a mapping. Instead the call's `arguments` is
        re-pointed at `{}` for RENDERING (so a delta can still be sent
        rather than crashing) while the original string survives under an
        internal `__raw_arguments__` key that the template never looks at
        but `_tool_call_identity` does -- so two genuinely different
        malformed payloads still compare unequal, and neither is ever
        misread as "no arguments."

        Never mutates the caller's dicts: builds new ones throughout.
        """
        if not isinstance(call, dict):
            return call, False
        new_call = dict(call)
        wrapped = isinstance(new_call.get("function"), dict)
        fn = dict(new_call["function"]) if wrapped else new_call
        arguments = fn.get("arguments")
        unparseable = False
        if isinstance(arguments, str):
            try:
                parsed = json.loads(arguments)
            except (json.JSONDecodeError, ValueError):
                parsed = None
            if isinstance(parsed, dict):
                fn["arguments"] = parsed
            else:
                unparseable = True
                fn["__raw_arguments__"] = arguments
                fn["arguments"] = {}
        if wrapped:
            new_call["function"] = fn
        else:
            new_call = fn
        return new_call, unparseable

    @staticmethod
    def _normalize_message(message: dict) -> tuple[dict, bool]:
        """A new copy of `message` with every tool call's `arguments`
        normalized to a mapping (see `_normalize_tool_call`), plus whether
        any call's `arguments` was an unparseable string. Non-assistant
        messages, and assistant messages without tool_calls, pass through
        as a plain per-message copy (already required by Finding 2)."""
        new_message = dict(message)
        tool_calls = new_message.get("tool_calls")
        if new_message.get("role") != "assistant" or not tool_calls:
            return new_message, False
        new_calls = []
        had_unparseable = False
        for call in tool_calls:
            normalized_call, unparseable = Session._normalize_tool_call(call)
            new_calls.append(normalized_call)
            had_unparseable = had_unparseable or unparseable
        new_message["tool_calls"] = new_calls
        return new_message, had_unparseable

    @staticmethod
    def _tool_call_identity(tool_calls) -> tuple:
        """The subset of a `tool_calls` list the template
        (`templates/qwen36-reap48-ours.jinja`, lines ~105-127) actually
        renders: each call's function `name` and `arguments`, in order
        (order-sensitive, since the template iterates `arguments|items` in
        dict order -- a reordering changes the rendered bytes). Never the
        call's `id` or `type`: the template never references either.

        Three spellings of "no tool calls" all render nothing and must
        collapse to the same identity: an absent field, an explicit `None`,
        and `[]` (falsy, so `not tool_calls` is True for every one of
        them). Likewise a call with `arguments` absent and one with
        `arguments: {}` both render zero `<parameter>` blocks, so both
        normalise to an empty tuple of argument pairs.

        Expects `tool_calls` to already be `_normalize_tool_call`-clean
        (arguments is always a dict, with `__raw_arguments__` set exactly
        when the original was an unparseable string). A `__raw_arguments__`
        marker is folded into the identity instead of the (empty) rendered
        `arguments`, so two DIFFERENT unparseable payloads -- both
        substituted with `{}` for rendering safety -- still compare
        unequal rather than being silently read as the same "no
        arguments" call.
        """
        if not tool_calls:
            return ()
        identities = []
        for call in tool_calls:
            if not isinstance(call, dict):
                identities.append((None, ()))
                continue
            fn = call.get("function", call)
            if not isinstance(fn, dict):
                fn = {}
            name = fn.get("name")
            if "__raw_arguments__" in fn:
                identities.append((name, ("__unparseable__", fn["__raw_arguments__"])))
                continue
            arguments = fn.get("arguments")
            args_items = tuple(arguments.items()) if isinstance(arguments, dict) else ()
            identities.append((name, args_items))
        return tuple(identities)

    @staticmethod
    def _reasoning_identity(message: dict):
        """The template only uses `reasoning_content` when it `is string`
        (line ~91); otherwise it derives reasoning by splitting `content`
        on `</think>`, a pure function of `content`, which is already
        compared separately. So an absent `reasoning_content` and an
        explicit `None` are equivalent (both fail `is string`, both fall
        back to the same content-derived value) and normalise to `None`
        here; any actual string -- including `""` -- is compared as
        itself, since only a real string overrides the content-derived
        reasoning."""
        reasoning_content = message.get("reasoning_content")
        return reasoning_content if isinstance(reasoning_content, str) else None

    @staticmethod
    def _is_extension(previous: list[dict], current: list[dict]) -> bool:
        # Full SEMANTIC message identity: everything the template actually
        # renders for a message, not a role/content subset. An assistant
        # turn carrying tool_calls has content "" or None, so two DIFFERENT
        # tool-call turns can compare equal on role+content alone. That
        # would misclassify a client's rewritten history as an append and
        # silently corrupt the KV. Conversely, fields the template never
        # renders (tool_calls[].id, tool_calls[].type) must NOT force a
        # reset merely because a client regenerated them. When in doubt,
        # this must refuse (reset) rather than accept: a false reset costs
        # performance, a false append costs correctness.
        if len(current) < len(previous):
            return False
        for old, new in zip(previous, current):
            if (old.get("role") != new.get("role")
                    or old.get("content") != new.get("content")
                    or Session._reasoning_identity(old) != Session._reasoning_identity(new)
                    or (Session._tool_call_identity(old.get("tool_calls"))
                        != Session._tool_call_identity(new.get("tool_calls")))):
                return False
        return True

    def next_delta(self, messages: list[dict], tools) -> tuple[str, bool]:
        """The bytes to send, and whether this was a reset.

        `was_reset` is True only when the client rewrote history rather than
        appending to it — never on the first turn of a session, which is
        simply the initial render.
        """
        # A per-message copy, not just a copy of the outer list: a caller
        # that mutates a message dict in place after this call must not be
        # able to silently desync `_sent_messages` from the bytes actually
        # sent (Finding 2). `_normalize_message` also coerces each tool
        # call's `arguments` to a mapping, accepting the JSON-string wire
        # shape the real OpenAI format (and Task 2's own parser) actually
        # uses, so it never reaches the template as a bare string.
        normalized = [Session._normalize_message(m) for m in messages]
        messages = [m for m, _ in normalized]
        had_unparseable_arguments = any(bad for _, bad in normalized)
        had_history = bool(self._sent_messages)
        # An unparseable `arguments` string makes this turn's messages
        # un-trustworthy as an append target: we cannot tell whether it
        # matches the resident KV, and appending onto a mismatch would be
        # silent corruption. Force the safe branch -- reset, not append --
        # rather than let `_is_extension`'s tuple comparison decide, since
        # `_tool_call_identity` deliberately makes an unparseable call
        # compare unequal to everything except a byte-identical repeat of
        # the same malformed string, which is exactly "we cannot be sure."
        is_extension = (had_history and not had_unparseable_arguments
                         and self._is_extension(self._sent_messages, messages))

        if is_extension and self.keep_reasoning:
            new = messages[len(self._sent_messages):]
            opener = "<|im_end|>\n" if self._open_assistant_turn else ""
            delta = opener + self.template.render_turns(new) + self.template.GENERATION_SUFFIX
            self._sent_messages = messages
            self._open_assistant_turn = True
            return delta, False

        # Full render path. This is reached for three distinct reasons:
        #   - first turn of the session (had_history is False);
        #   - the client rewrote history rather than appending (a real
        #     divergence: is_extension is False despite had_history);
        #   - keep_reasoning=False on an otherwise-legitimate append (spec's
        #     option B: no reuse, by deliberate choice, not a divergence).
        # Only the middle case is a reset -- the client did something the
        # adapter cannot trust. The third case is the adapter choosing not
        # to reuse a resident KV it could have reused; that is not the
        # client's fault, so was_reset must stay False.
        delta = self.template.render_initial(messages, tools)
        self._sent_messages = messages
        self._open_assistant_turn = True
        was_reset = had_history and not is_extension
        return delta, was_reset

    def record_generation(self, raw: str) -> None:
        """The generated bytes are already in the KV; record that the
        assistant turn is open so the next delta closes it."""
        self._sent_messages = self._sent_messages + [{"role": "assistant", "content": raw}]
        self._open_assistant_turn = True
