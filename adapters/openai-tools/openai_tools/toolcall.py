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

"""Parsing the model's trained tool-call format into OpenAI `tool_calls`.

The adapter never fabricates. Anything that does not parse cleanly and
completely returns `None`, and the caller surfaces the model's text as
assistant content instead — guessing here would reproduce the silent-failure
class this whole project exists to refuse.

PARSING STRATEGY — a strict left-to-right scanner, not regex-plus-checks.

Earlier revisions matched pieces with regexes and then tried to verify,
after the fact, that nothing had been silently skipped (byte-coverage
checks, delimiter-count checks). Every one of those checks was a specific
patch for a specific bypass, and each left another bypass a level away —
a duplicate parameter key defeats coverage checks because coverage is
byte-level, not semantic; an unterminated `<tool_call>` defeats a
`findall`-based scan because `findall` only ever looks at matched pairs.

This version instead walks the input once with an explicit cursor. At
every point the cursor is either advanced by consuming exactly one
grammar construct (a literal delimiter, an identifier, a value up to its
terminator) or the whole parse is refused. There is no code path that
moves the cursor without accounting for what it passed over, so
losslessness does not need to be checked separately — it is a structural
property of the scanner, not an invariant maintained by side checks.

Only the interior of `<tool_call>...</tool_call>` blocks is scanned this
way. Text outside those blocks (reasoning, prose before or between calls)
is the model's legitimate natural-language output and is left untouched;
the caller is responsible for preserving it as message content.

Grammar (informal), matching the trained template:

    top    ::= (PROSE? "<tool_call>" block "</tool_call>")*  PROSE?
    block  ::= WS* "<function=" IDENT ">" body "</function>" WS*
    body   ::= (WS* "<parameter=" IDENT ">" VALUE "</parameter>")* WS*

`VALUE` runs up to the next literal `</parameter>` — the format has no
escaping mechanism, so that is the only value a terminator can have.  If
that leaves un-accounted-for, non-whitespace text at any level (a second
`<function=...>` in one block, a fake tag balancing another one's count,
junk before or after a construct, a duplicate parameter key, a
`<tool_call>` with no closer at all) the corresponding parse step simply
does not succeed, and the scanner reports `None` for the whole call — not
a null for just that piece.
"""
import json
import re
import uuid

_TOOL_OPEN = "<tool_call>"
_TOOL_CLOSE = "</tool_call>"
_FUNC_OPEN = "<function="
_FUNC_CLOSE = "</function>"
_PARAM_OPEN = "<parameter="
_PARAM_CLOSE = "</parameter>"

_IDENT = re.compile(r"[A-Za-z0-9_.-]+")


def split_reasoning(raw: str) -> tuple[str, str]:
    """Generation starts inside `<think>`, so split the reasoning off first."""
    if "</think>" in raw:
        reasoning, _, visible = raw.partition("</think>")
        return reasoning.strip("\n").removeprefix("<think>").strip("\n"), visible.strip("\n")
    return "", raw.strip("\n")


def _schema_for(tools, name):
    for tool in tools or []:
        fn = tool.get("function", {})
        if fn.get("name") == name:
            return fn.get("parameters", {}).get("properties", {})
    return None


def _coerce(value: str, declared: dict):
    kind = (declared or {}).get("type", "string")
    if kind == "string":
        return value
    if kind == "integer":
        return int(value)
    if kind == "number":
        return float(value)
    if kind == "boolean":
        low = value.strip().lower()
        if low in ("true", "false"):
            return low == "true"
        raise ValueError(f"not a boolean: {value!r}")
    if kind in ("object", "array"):
        return json.loads(value)
    return value


def _skip_ws(text: str, pos: int) -> int:
    n = len(text)
    while pos < n and text[pos].isspace():
        pos += 1
    return pos


def _find_unambiguous_close(text: str, start: int, open_token: str, close_token: str):
    """Find `close_token` after `start`, but only if it is trustworthy.

    None of `<tool_call>`, `<function=`, `<parameter=` nest in this
    grammar. So if `open_token` reappears before the first `close_token`
    we find, we cannot tell whether that close belongs to the construct
    we are currently inside or to the one that just re-opened — treating
    it as ours would silently fold everything past the real close (which
    a value may legitimately contain, e.g. the model quoting an example)
    into "outside the construct", where it is never looked at again.
    Refusing here, rather than guessing the pairing, is what keeps that
    content from vanishing.
    """
    close_at = text.find(close_token, start)
    if close_at == -1:
        return None
    next_open = text.find(open_token, start)
    if next_open != -1 and next_open < close_at:
        return None
    return close_at


def _strip_one_newline(value: str) -> str:
    """Undo the single blank line the template puts around a tag's content.

    Mirrors the old `\\n?VALUE\\n?` shape without regex: at most one
    leading and one trailing newline are template formatting, not part of
    the value the model meant to send.
    """
    if value.startswith("\n"):
        value = value[1:]
    if value.endswith("\n"):
        value = value[:-1]
    return value


def _parse_params(body: str, properties: dict):
    """Consume `body` as a sequence of `<parameter=...>` entries.

    Returns the argument dict, or `None` if any part of `body` is not
    whitespace and not a complete, uniquely-keyed, schema-declared
    parameter entry — including a parameter value that runs off the end
    of `body` with no `</parameter>` to close it.
    """
    n = len(body)
    pos = 0
    args: dict = {}
    while True:
        pos = _skip_ws(body, pos)
        if pos == n:
            return args
        if not body.startswith(_PARAM_OPEN, pos):
            return None  # non-whitespace content that isn't a parameter tag
        pos += len(_PARAM_OPEN)

        match = _IDENT.match(body, pos)
        if match is None:
            return None  # no identifier after <parameter=
        key = match.group(0)
        pos = match.end()

        if pos >= n or body[pos] != ">":
            return None  # unclosed opening tag, e.g. <parameter=name /
        pos += 1

        value_start = pos
        close_at = _find_unambiguous_close(body, value_start, _PARAM_OPEN, _PARAM_CLOSE)
        if close_at is None:
            return None  # unterminated, or an ambiguous nested <parameter=...>
        raw_value = _strip_one_newline(body[value_start:close_at])
        pos = close_at + len(_PARAM_CLOSE)

        if key in args:
            return None  # duplicate key: the earlier value would be silently lost
        if key not in properties:
            return None  # a parameter the caller's schema never declared
        try:
            args[key] = _coerce(raw_value, properties.get(key))
        except (ValueError, json.JSONDecodeError):
            return None  # loudly, rather than passing a wrong type


def _parse_block(block: str, tools):
    """Consume the interior of one `<tool_call>...</tool_call>` pair.

    Returns `(name, args)`, or `None` if the block is not exactly one
    whitespace-padded `<function=...>...</function>` construct whose body
    fully parses as parameters.
    """
    pos = _skip_ws(block, 0)
    if not block.startswith(_FUNC_OPEN, pos):
        return None  # junk before the function tag, or no function at all
    pos += len(_FUNC_OPEN)

    match = _IDENT.match(block, pos)
    if match is None:
        return None
    name = match.group(0)
    pos = match.end()

    if pos >= len(block) or block[pos] != ">":
        return None
    pos += 1

    body_start = pos
    close_at = _find_unambiguous_close(block, body_start, _FUNC_OPEN, _FUNC_CLOSE)
    if close_at is None:
        return None  # unterminated, or an ambiguous nested <function=...>
    body = block[body_start:close_at]
    pos = close_at + len(_FUNC_CLOSE)

    pos = _skip_ws(block, pos)
    if pos != len(block):
        return None  # trailing junk, or a second <function=...> in the same block

    properties = _schema_for(tools, name)
    if properties is None:
        return None  # a name the caller never offered

    args = _parse_params(body, properties)
    if args is None:
        return None
    return name, args


def parse_tool_calls(visible: str, tools):
    """Scan `visible` left to right for `<tool_call>` blocks.

    Every `<tool_call>` opener must be matched by a later `</tool_call>`
    and its interior must parse as exactly one well-formed function call
    (see `_parse_block`); any failure anywhere refuses the *entire*
    result rather than returning a partial list, so a caller can never
    silently receive some calls while others vanish. Text outside
    `<tool_call>` blocks — prose before, between, or after calls — is not
    inspected; it is the model's legitimate natural-language output.
    """
    text = visible or ""
    pos = 0
    calls = []
    while True:
        open_at = text.find(_TOOL_OPEN, pos)
        if open_at == -1:
            break

        block_start = open_at + len(_TOOL_OPEN)
        close_at = _find_unambiguous_close(text, block_start, _TOOL_OPEN, _TOOL_CLOSE)
        if close_at is None:
            # Either no closer at all — e.g. generation cut by max_tokens —
            # or a second <tool_call> opened before this one closed, which
            # this grammar does not support and cannot safely pair.
            return None

        parsed = _parse_block(text[block_start:close_at], tools)
        if parsed is None:
            return None
        name, args = parsed
        calls.append({
            "id": f"call_{uuid.uuid4().hex[:24]}",
            "type": "function",
            "function": {"name": name, "arguments": json.dumps(args)},
        })
        pos = close_at + len(_TOOL_CLOSE)

    return calls or None
