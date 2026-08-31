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

        # Finding 2: Detect truncated parameter values containing </parameter>
        if body.count("<parameter=") != body.count("</parameter>"):
            return None  # malformed parameters, likely truncated value

        args = {}
        for key, raw_value in _PARAM.findall(body):
            # Finding 3: Reject undeclared parameters
            if key not in properties:
                return None  # parameter key not in schema
            try:
                args[key] = _coerce(raw_value, properties.get(key))
            except (ValueError, json.JSONDecodeError):
                return None  # loudly, rather than passing a wrong type
        calls.append({
            "id": f"call_{uuid.uuid4().hex[:24]}",
            "type": "function",
            "function": {"name": name, "arguments": json.dumps(args)},
        })
    return calls or None
