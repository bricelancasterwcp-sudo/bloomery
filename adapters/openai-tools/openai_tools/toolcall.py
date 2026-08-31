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
"""
import json
import re
import uuid

_CALL = re.compile(r"<tool_call>\s*(.*?)\s*</tool_call>", re.DOTALL)
_FUNC = re.compile(r"<function=([A-Za-z0-9_.-]+)>\s*(.*?)\s*</function>", re.DOTALL)
_PARAM = re.compile(r"<parameter=([A-Za-z0-9_.-]+)>\n?(.*?)\n?</parameter>", re.DOTALL)


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


def parse_tool_calls(visible: str, tools):
    blocks = _CALL.findall(visible or "")
    if not blocks:
        return None
    calls = []
    for block in blocks:
        # Finding 1: Require exactly one function per block
        funcs = _FUNC.findall(block)
        if len(funcs) != 1:
            return None  # zero or more than one function per block
        name, body = funcs[0]
        properties = _schema_for(tools, name)
        if properties is None:
            return None  # a name the caller never offered

        # Finding 2 (block level): Verify block is fully consumed by the function.
        # The function match must cover all non-whitespace content in the block.
        func_match = _FUNC.search(block)
        if not func_match:
            return None  # Should never happen given findall check above
        func_start, func_end = func_match.span()
        # Check that everything outside the function span in the block is whitespace
        for i, char in enumerate(block):
            if (i < func_start or i >= func_end) and not char.isspace():
                return None  # non-whitespace outside function span in block

        args = {}
        for key, raw_value in _PARAM.findall(body):
            # Finding 3: Reject undeclared parameters
            if key not in properties:
                return None  # parameter key not in schema
            try:
                coerced = _coerce(raw_value, properties.get(key))
            except (ValueError, json.JSONDecodeError):
                return None  # loudly, rather than passing a wrong type
            # Finding 2 (parameter level): Enforce losslessness — reject values containing
            # <parameter= or </parameter>, as these substrings cannot occur in a
            # well-formed value and indicate the delimiters are ambiguous.
            if isinstance(coerced, str) and ("<parameter=" in coerced or "</parameter>" in coerced):
                return None  # ambiguous delimiters, value cannot be trusted
            args[key] = coerced

        # Finding 2 (parameter-body level): Verify the function body is fully consumed
        # by the parameter blocks. Check if everything outside those blocks is
        # whitespace-only. This catches truncated values and malformed parameter tags.
        covered = set()
        for match in _PARAM.finditer(body):
            start, end = match.span()
            covered.add((start, end))

        # Mark covered ranges
        is_covered = [False] * len(body)
        for start, end in covered:
            for i in range(start, end):
                is_covered[i] = True

        # Check that everything outside covered ranges is whitespace
        for i, char in enumerate(body):
            if not is_covered[i] and not char.isspace():
                return None  # non-whitespace outside parameter blocks

        calls.append({
            "id": f"call_{uuid.uuid4().hex[:24]}",
            "type": "function",
            "function": {"name": name, "arguments": json.dumps(args)},
        })
    return calls or None
