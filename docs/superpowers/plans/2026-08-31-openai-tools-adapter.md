# OpenAI Tools Adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Python sidecar that lets an OpenAI tool-calling client (hermes) drive a local model served by bloomery, by owning chat templating, tool-schema rendering and tool-call parsing — the things the daemon deliberately refuses to do.

**Architecture:** An HTTP server speaking the OpenAI chat-completions API upstream and bloomery's **native** `/agents/{id}/infer` downstream. Prompts are rendered through the model's own GGUF chat template. One client session maps to one persistent bloomery agent; conversation state is maintained by **incremental append** (never re-rendering history), so the ~21,931-token tool-schema preamble is prefilled once per session instead of every turn.

**Tech Stack:** Python 3.14, stdlib `http.server` + `urllib.request` (no web framework — none is installed and the repo's Python is dependency-light), `jinja2` 3.1.6 (present), stdlib `unittest` (**pytest is NOT importable on this box**), `gguf` via `PYTHONPATH=~/llama.cpp/gguf-py` for one-time template extraction only.

**Spec:** `docs/superpowers/specs/2026-08-31-openai-tools-adapter-design.md` — including its **2026-08-31 amendment to §6**, which withdraws full-render diffing in favour of incremental append. Read the amendment before Task 3; it is the reason that task exists in the shape it does.

## Global Constraints

- **Tests are stdlib `unittest`.** `python3 -m unittest discover -s adapters/openai-tools/tests -t .` from the repo root. Never `pytest` — it is not importable here.
- **No new runtime dependency beyond `jinja2`.** `gguf` is a build-time tool only; the adapter reads a committed `.jinja` file at runtime.
- **No daemon change.** This plan touches nothing under `crates/`.
- **The adapter never fabricates.** A tool call that does not parse is returned as assistant `content`. No guessing, no retry-until-parses.
- **Every delta sent to bloomery begins at `<|im_start|>`** (a special-token boundary) or is the initial full render. This is the token-safety rule from spec §6; tokenization does not distribute over concatenation, so a split anywhere else is a defect.
- **Model:** `qwen36-reap48-ours`, GGUF at `/home/brice/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf`, `kv_per_token` 20,480, template sha256 begins `e84f32a23fdda276`, length 7,764 bytes.
- Licence header on every new file: AGPL-3.0-only, matching `tools/*/__init__.py`.

---

### Task 1: Template extraction and rendering

**Files:**
- Create: `adapters/openai-tools/tools/extract_template.py`
- Create: `adapters/openai-tools/templates/qwen36-reap48-ours.jinja`
- Create: `adapters/openai-tools/openai_tools/__init__.py`
- Create: `adapters/openai-tools/openai_tools/template.py`
- Test: `adapters/openai-tools/tests/test_template.py`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `ChatTemplate.load(path: str) -> ChatTemplate`
  - `ChatTemplate.render_initial(messages: list[dict], tools: list[dict]) -> str`
  - `ChatTemplate.render_turns(messages: list[dict]) -> str`
  - `ChatTemplate.GENERATION_SUFFIX: str` — the exact bytes `'<|im_start|>assistant\n<think>\n'`
  - `ChatTemplate.sha256: str`

**Why the wrapper is derived, not written.** The per-turn format looks like plain ChatML, but hand-writing it would silently drift if the model or template changes. `render_turns` therefore renders the template twice and takes the difference, and a test pins that the derived result equals the template's own output.

- [ ] **Step 1: Write the failing test**

```python
# adapters/openai-tools/tests/test_template.py
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -m unittest discover -s adapters/openai-tools/tests -t . -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'openai_tools'`

- [ ] **Step 3: Extract the template artifact**

```python
# adapters/openai-tools/tools/extract_template.py
"""One-time: lift `tokenizer.chat_template` out of a GGUF into a file.

Needs the `gguf` package, which on this box lives beside llama.cpp:

    PYTHONPATH=~/llama.cpp/gguf-py python3 adapters/openai-tools/tools/extract_template.py \\
        /home/brice/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf \\
        adapters/openai-tools/templates/qwen36-reap48-ours.jinja

The adapter itself never imports `gguf`: it reads the committed file, so the
template is a versioned, sha-pinned artifact rather than something re-derived
at every boot.
"""
import hashlib
import sys

import gguf


def main(gguf_path: str, out_path: str) -> int:
    reader = gguf.GGUFReader(gguf_path)
    for field in reader.fields.values():
        if "chat_template" in field.name:
            text = bytes(field.parts[field.data[0]]).decode("utf-8")
            with open(out_path, "w", encoding="utf-8") as handle:
                handle.write(text)
            print(f"wrote {out_path} ({len(text)} chars)")
            print("sha256:", hashlib.sha256(text.encode("utf-8")).hexdigest())
            return 0
    print("no chat_template in", gguf_path, file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1], sys.argv[2]))
```

Run it once to produce the committed artifact:

```bash
PYTHONPATH=~/llama.cpp/gguf-py python3 adapters/openai-tools/tools/extract_template.py \
    /home/brice/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf \
    adapters/openai-tools/templates/qwen36-reap48-ours.jinja
```

Expected output includes `sha256: e84f32a23fdda276...` and `(7764 chars)`.

- [ ] **Step 4: Write minimal implementation**

```python
# adapters/openai-tools/openai_tools/template.py
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
```

`adapters/openai-tools/openai_tools/__init__.py` carries the AGPL header and nothing else.

- [ ] **Step 5: Run test to verify it passes**

Run: `python3 -m unittest discover -s adapters/openai-tools/tests -t . -v`
Expected: 4 tests PASS

- [ ] **Step 6: Commit**

```bash
git add adapters/openai-tools/
git commit -m "feat: adapter template rendering, wrapper derived from the template"
```

---

### Task 2: Tool-call parsing

**Files:**
- Create: `adapters/openai-tools/openai_tools/toolcall.py`
- Test: `adapters/openai-tools/tests/test_toolcall.py`

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces:
  - `split_reasoning(raw: str) -> tuple[str, str]` returning `(reasoning, visible)`
  - `parse_tool_calls(visible: str, tools: list[dict]) -> list[dict] | None` — OpenAI `tool_calls` entries, or `None` when nothing parses cleanly

**The format**, as the template instructs the model to emit it:

```
<tool_call>
<function=terminal>
<parameter=command>
ls /tmp
</parameter>
</function>
</tool_call>
```

**Generation begins inside a reasoning block** — the prompt ends `<|im_start|>assistant\n<think>\n` — so raw output must be split on `</think>` before anything else is attempted.

- [ ] **Step 1: Write the failing test**

```python
# adapters/openai-tools/tests/test_toolcall.py
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


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -m unittest adapters.openai-tools.tests.test_toolcall -v` — or the discover command from Global Constraints.
Expected: FAIL — `ModuleNotFoundError: No module named 'openai_tools.toolcall'`

- [ ] **Step 3: Write minimal implementation**

```python
# adapters/openai-tools/openai_tools/toolcall.py
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
        func = _FUNC.search(block)
        if not func:
            return None
        name, body = func.group(1), func.group(2)
        properties = _schema_for(tools, name)
        if properties is None:
            return None  # a name the caller never offered
        args = {}
        for key, raw_value in _PARAM.findall(body):
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python3 -m unittest discover -s adapters/openai-tools/tests -t . -v`
Expected: all Task 1 and Task 2 tests PASS

- [ ] **Step 5: Mutation check**

A parser that accepts everything passes every positive test. Verify the negative tests are load-bearing:

```bash
# Mutant: make an unknown function name pass through instead of refusing.
# Change `return None  # a name the caller never offered` to `properties = {}`
# Expected: test_an_unknown_function_name_returns_none FAILS, and no other test does.
# Then revert.
```

Record the result in the commit message. If a mutant survives, the negative test is not testing what it claims.

- [ ] **Step 6: Commit**

```bash
git add adapters/openai-tools/
git commit -m "feat: tool-call parsing with schema coercion, refusing rather than guessing"
```

---

### Task 3: Session state and the delta

**Files:**
- Create: `adapters/openai-tools/openai_tools/session.py`
- Test: `adapters/openai-tools/tests/test_session.py`

**Read spec §6 and its 2026-08-31 amendment before starting.** This task exists in this shape because the template rewrites historical assistant turns, so full-render diffing would reset the agent every turn.

**Interfaces:**
- Consumes: `ChatTemplate` from Task 1.
- Produces:
  - `Session(agent_id: str, template: ChatTemplate, keep_reasoning: bool = True)`
  - `Session.next_delta(messages: list[dict], tools: list[dict]) -> tuple[str, bool]` returning `(delta, was_reset)`
  - `Session.record_generation(raw: str) -> None`

**The rule.** The adapter tracks the byte string the agent's KV holds. Turn 1 is a full render. After generation, the raw generated bytes are appended to that record, because bloomery has already fed them into the KV. Each later turn sends `<|im_end|>\n` + the new turns + the generation prompt. Divergence is a **semantic** check on the incoming message list: if the client edited or compressed history rather than appending, reset.

- [ ] **Step 1: Write the failing test**

```python
# adapters/openai-tools/tests/test_session.py
import unittest

from openai_tools.session import Session
from openai_tools.template import ChatTemplate

TPL = "adapters/openai-tools/templates/qwen36-reap48-ours.jinja"
TOOLS = [{"type": "function", "function": {
    "name": "terminal", "parameters": {"type": "object", "properties": {}}}}]
U1 = {"role": "user", "content": "first"}
U2 = {"role": "user", "content": "second"}
GEN = "\nreasoning\n</think>\n\nan answer"


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


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -m unittest discover -s adapters/openai-tools/tests -t . -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'openai_tools.session'`

- [ ] **Step 3: Write minimal implementation**

```python
# adapters/openai-tools/openai_tools/session.py
"""One client session, one bloomery agent, and the bytes its KV holds.

bloomery APPENDS: `generate_from` starts at `kv_cache_seq_pos_max + 1` with
`AddBos::Never`. Re-sending a conversation would duplicate it into the cache,
so this class exists to send only what is new — and, per the spec's
2026-08-31 amendment, to do that WITHOUT re-rendering history, because the
template rewrites historical assistant turns and a re-render is therefore not
a prefix extension of what was actually sent.
"""


class Session:
    def __init__(self, agent_id: str, template, keep_reasoning: bool = True):
        self.agent_id = agent_id
        self.template = template
        self.keep_reasoning = keep_reasoning
        self._sent_messages: list[dict] = []
        self._open_assistant_turn = False

    @staticmethod
    def _is_extension(previous: list[dict], current: list[dict]) -> bool:
        if len(current) < len(previous):
            return False
        for old, new in zip(previous, current):
            if old.get("role") != new.get("role") or old.get("content") != new.get("content"):
                return False
        return True

    def next_delta(self, messages: list[dict], tools) -> tuple[str, bool]:
        messages = list(messages)
        if self._sent_messages and self._is_extension(self._sent_messages, messages):
            new = messages[len(self._sent_messages):]
            opener = "<|im_end|>\n" if self._open_assistant_turn else ""
            delta = opener + self.template.render_turns(new) + self.template.GENERATION_SUFFIX
            self._sent_messages = messages
            self._open_assistant_turn = True
            return delta, False

        # First turn, or the client rewrote history: start clean.
        delta = self.template.render_initial(messages, tools)
        self._sent_messages = messages
        self._open_assistant_turn = True
        return delta, bool(self._sent_messages) and False if not messages else (delta, False)[1] or self._was_reset(messages)

    def _was_reset(self, messages) -> bool:
        return self._reset_flag

    def record_generation(self, raw: str) -> None:
        """The generated bytes are already in the KV; record that the
        assistant turn is open so the next delta closes it."""
        self._sent_messages = self._sent_messages + [{"role": "assistant", "content": raw}]
        self._open_assistant_turn = True
```

> **Implementer note:** the `next_delta` return above is deliberately left
> awkward — write it cleanly. The contract is `(delta, was_reset)` where
> `was_reset` is `True` only on the history-rewritten path, `False` on the
> very first turn of a session. Let the tests drive the shape; do not copy
> the expression verbatim.

- [ ] **Step 4: Run test to verify it passes**

Run: `python3 -m unittest discover -s adapters/openai-tools/tests -t . -v`
Expected: all PASS

- [ ] **Step 5: Mutation check**

A `next_delta` that always returns the full render is correct-looking and destroys the entire KV benefit — the reason this adapter exists. Verify the tests catch it:

```bash
# Mutant: make next_delta always take the render_initial path.
# Expected: test_second_turn_sends_only_the_new_turn_not_the_preamble FAILS
#           and test_every_delta_begins_at_a_special_token_boundary FAILS.
# Then revert.
```

- [ ] **Step 6: Commit**

```bash
git add adapters/openai-tools/
git commit -m "feat: incremental-append session state per the spec's 08-31 amendment"
```

---

### Task 4: bloomery client and error mapping

**Files:**
- Create: `adapters/openai-tools/openai_tools/bloomery.py`
- Create: `adapters/openai-tools/openai_tools/errors.py`
- Test: `adapters/openai-tools/tests/test_errors.py`
- Test: `adapters/openai-tools/tests/test_bloomery.py`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `BloomeryClient(base_url: str)` with `create_agent(model, window_cap=None) -> str`, `infer(agent_id, prompt, max_tokens) -> dict`, `suspend(agent_id) -> None`
  - `BloomeryError(status: int, body: dict)`
  - `to_openai_error(err: BloomeryError) -> tuple[int, dict]`

**The mapping**, from `api_native.rs`'s own table:

| bloomery | HTTP | OpenAI |
|---|---|---|
| `prompt_too_large` | 413 | `context_length_exceeded`, keeping `needed_tokens` / `window_tokens` |
| `refused` | 409 | `server_error`, keeping needed / free / reclaimable |
| `budget` | 402 | `insufficient_quota` |
| `unknown_model` | 404 | `model_not_found` |
| `contract` | 502 | `server_error` |

- [ ] **Step 1: Write the failing test**

```python
# adapters/openai-tools/tests/test_errors.py
import unittest

from openai_tools.errors import BloomeryError, to_openai_error


class ErrorMappingTest(unittest.TestCase):
    def test_oversize_becomes_context_length_exceeded_and_keeps_the_arithmetic(self):
        status, body = to_openai_error(BloomeryError(413, {
            "error": "prompt_too_large", "needed_tokens": 120000,
            "window_tokens": 103124}))
        self.assertEqual(status, 413)
        self.assertEqual(body["error"]["code"], "context_length_exceeded")
        self.assertIn("120000", body["error"]["message"])
        self.assertIn("103124", body["error"]["message"])

    def test_residency_refusal_keeps_the_bytes_it_was_refused_over(self):
        status, body = to_openai_error(BloomeryError(409, {
            "error": "refused", "needed": 2611945472, "free": 1925343488,
            "reclaimable": 0}))
        self.assertEqual(status, 409)
        self.assertIn("2611945472", body["error"]["message"])

    def test_budget_exhaustion_is_insufficient_quota(self):
        status, body = to_openai_error(BloomeryError(402, {"error": "budget"}))
        self.assertEqual(status, 402)
        self.assertEqual(body["error"]["code"], "insufficient_quota")

    def test_an_unmapped_status_is_surfaced_not_swallowed(self):
        status, body = to_openai_error(BloomeryError(500, {"error": "weird"}))
        self.assertEqual(status, 502)
        self.assertIn("weird", body["error"]["message"])


if __name__ == "__main__":
    unittest.main()
```

```python
# adapters/openai-tools/tests/test_bloomery.py
import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from openai_tools.bloomery import BloomeryClient
from openai_tools.errors import BloomeryError


class _Stub(BaseHTTPRequestHandler):
    routes = {}

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        self.rfile.read(length)
        status, payload = self.routes.get(self.path, (404, {"error": "no route"}))
        raw = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def log_message(self, *args):
        pass


def _serve(routes):
    _Stub.routes = routes
    srv = ThreadingHTTPServer(("127.0.0.1", 0), _Stub)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv


class BloomeryClientTest(unittest.TestCase):
    def test_create_agent_returns_the_id_the_daemon_assigned(self):
        srv = _serve({"/agents": (200, {"id": "a7", "window_tokens": 103124,
                                        "bound_by": "vram"})})
        try:
            client = BloomeryClient(f"http://127.0.0.1:{srv.server_port}")
            self.assertEqual(client.create_agent("m"), "a7")
        finally:
            srv.shutdown()

    def test_infer_returns_the_reply_body(self):
        srv = _serve({"/agents/a7/infer": (200, {
            "text": "hello", "prompt_tokens": 8, "completion_tokens": 2,
            "duration_ms": 12})})
        try:
            client = BloomeryClient(f"http://127.0.0.1:{srv.server_port}")
            self.assertEqual(client.infer("a7", "p", 16)["text"], "hello")
        finally:
            srv.shutdown()

    def test_a_refusal_is_raised_as_BloomeryError_carrying_the_body(self):
        srv = _serve({"/agents/a7/infer": (413, {
            "error": "prompt_too_large", "needed_tokens": 9, "window_tokens": 4})})
        try:
            client = BloomeryClient(f"http://127.0.0.1:{srv.server_port}")
            with self.assertRaises(BloomeryError) as caught:
                client.infer("a7", "p", 16)
            self.assertEqual(caught.exception.status, 413)
            self.assertEqual(caught.exception.body["needed_tokens"], 9)
        finally:
            srv.shutdown()


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -m unittest discover -s adapters/openai-tools/tests -t . -v`
Expected: FAIL — `ModuleNotFoundError` for `openai_tools.errors` and `openai_tools.bloomery`

- [ ] **Step 3: Write minimal implementation**

```python
# adapters/openai-tools/openai_tools/errors.py
"""bloomery refusals, translated without losing the arithmetic.

bloomery's refusals carry numbers — bytes needed and free, tokens needed and
available. Those numbers are the point: a client that receives them can act,
where a bare 500 leaves it guessing. Nothing here rounds them off.
"""


class BloomeryError(Exception):
    def __init__(self, status: int, body: dict):
        super().__init__(f"bloomery {status}: {body}")
        self.status = status
        self.body = body or {}


def _envelope(kind: str, code: str, message: str) -> dict:
    return {"error": {"type": kind, "code": code, "message": message}}


def to_openai_error(err: BloomeryError) -> tuple[int, dict]:
    body, kind = err.body, err.body.get("error", "")
    if err.status == 413 or kind == "prompt_too_large":
        return 413, _envelope(
            "invalid_request_error", "context_length_exceeded",
            f"prompt needs {body.get('needed_tokens')} tokens; the agent's window "
            f"is {body.get('window_tokens')}. bloomery refuses rather than truncating.")
    if err.status == 409 or kind == "refused":
        return 409, _envelope(
            "server_error", "residency_refused",
            f"the model could not be made resident: needed {body.get('needed')} B, "
            f"free {body.get('free')} B, reclaimable {body.get('reclaimable')} B.")
    if err.status == 402 or kind == "budget":
        return 402, _envelope("insufficient_quota", "insufficient_quota",
                              f"token budget exhausted: {body}")
    if err.status == 404:
        return 404, _envelope("invalid_request_error", "model_not_found", f"{body}")
    return 502, _envelope("server_error", "upstream_error",
                          f"bloomery returned {err.status}: {body}")
```

```python
# adapters/openai-tools/openai_tools/bloomery.py
"""A small client for bloomery's native agent API.

Native rather than `/v1`: the adapter must own the prompt bytes exactly, and
`/v1`'s `fallback_prompt` would rewrite them into `"role: content"` lines.
"""
import json
import urllib.error
import urllib.request

from .errors import BloomeryError


class BloomeryClient:
    def __init__(self, base_url: str, timeout: float = 600.0):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    def _post(self, path: str, payload: dict) -> dict:
        raw = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(
            f"{self.base_url}{path}", data=raw,
            headers={"Content-Type": "application/json"}, method="POST")
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                text = resp.read().decode("utf-8")
                return json.loads(text) if text else {}
        except urllib.error.HTTPError as exc:
            text = exc.read().decode("utf-8", errors="replace")
            try:
                body = json.loads(text)
            except json.JSONDecodeError:
                body = {"error": text}
            raise BloomeryError(exc.code, body) from exc

    def create_agent(self, model: str, window_cap: int | None = None) -> str:
        payload: dict = {"model": model}
        if window_cap is not None:
            payload["window_cap"] = window_cap
        return self._post("/agents", payload)["id"]

    def infer(self, agent_id: str, prompt: str, max_tokens: int) -> dict:
        return self._post(f"/agents/{agent_id}/infer",
                          {"prompt": prompt, "max_tokens": max_tokens})

    def suspend(self, agent_id: str) -> None:
        self._post(f"/agents/{agent_id}/suspend", {})
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python3 -m unittest discover -s adapters/openai-tools/tests -t . -v`
Expected: all PASS

- [ ] **Step 5: Commit**

```bash
git add adapters/openai-tools/
git commit -m "feat: bloomery native client and refusal mapping that keeps the arithmetic"
```

---

### Task 5: The HTTP server, wired end to end

**Files:**
- Create: `adapters/openai-tools/openai_tools/server.py`
- Create: `adapters/openai-tools/README.md`
- Create: `adapters/openai-tools/config.example.json`
- Test: `adapters/openai-tools/tests/test_server.py`

**Interfaces:**
- Consumes: everything from Tasks 1–4.
- Produces: `build_server(config: dict, client) -> ThreadingHTTPServer` and a `__main__` entry point.

The integration test drives the whole loop against a **stub bloomery** — no model, no GPU — matching the daemon's own discipline that its 911 tests need neither.

- [ ] **Step 1: Write the failing test**

```python
# adapters/openai-tools/tests/test_server.py
import json
import threading
import unittest
import urllib.request

from openai_tools.server import build_server

TOOLS = [{"type": "function", "function": {
    "name": "terminal", "parameters": {"type": "object",
                                       "properties": {"command": {"type": "string"}}}}}]
CALL = ("\nthinking\n</think>\n\n<tool_call>\n<function=terminal>\n"
        "<parameter=command>\nls /tmp\n</parameter>\n</function>\n</tool_call>")


class _FakeBloomery:
    """Stands in for the daemon: records prompts, returns a scripted reply."""

    def __init__(self, reply):
        self.reply = reply
        self.prompts = []

    def create_agent(self, model, window_cap=None):
        return "a1"

    def infer(self, agent_id, prompt, max_tokens):
        self.prompts.append(prompt)
        return {"text": self.reply, "prompt_tokens": 10,
                "completion_tokens": 5, "duration_ms": 3}

    def suspend(self, agent_id):
        pass


def _post(port, payload):
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/v1/chat/completions",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"}, method="POST")
    with urllib.request.urlopen(req, timeout=10) as resp:
        return resp.status, json.loads(resp.read().decode())


class ServerTest(unittest.TestCase):
    def _serve(self, reply):
        fake = _FakeBloomery(reply)
        cfg = {"model": "m",
               "template": "adapters/openai-tools/templates/qwen36-reap48-ours.jinja",
               "max_tokens": 64}
        srv = build_server(cfg, fake)
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        self.addCleanup(srv.shutdown)
        return srv.server_port, fake

    def test_a_tool_call_round_trips_into_openai_shape(self):
        port, fake = self._serve(CALL)
        status, body = _post(port, {"model": "m", "tools": TOOLS,
                                    "messages": [{"role": "user", "content": "ls"}]})
        self.assertEqual(status, 200)
        choice = body["choices"][0]
        self.assertEqual(choice["finish_reason"], "tool_calls")
        call = choice["message"]["tool_calls"][0]
        self.assertEqual(call["function"]["name"], "terminal")
        self.assertEqual(json.loads(call["function"]["arguments"]),
                         {"command": "ls /tmp"})

    def test_the_tool_schemas_reach_the_model(self):
        port, fake = self._serve(CALL)
        _post(port, {"model": "m", "tools": TOOLS,
                     "messages": [{"role": "user", "content": "ls"}]})
        self.assertIn("<tools>", fake.prompts[0])
        self.assertIn("terminal", fake.prompts[0])

    def test_unparseable_output_is_returned_as_content_never_as_a_fake_call(self):
        port, _ = self._serve("\nhm\n</think>\n\nI cannot do that.")
        status, body = _post(port, {"model": "m", "tools": TOOLS,
                                    "messages": [{"role": "user", "content": "ls"}]})
        choice = body["choices"][0]
        self.assertEqual(choice["finish_reason"], "stop")
        self.assertNotIn("tool_calls", choice["message"])
        self.assertIn("cannot", choice["message"]["content"])

    def test_the_preamble_is_sent_once_across_two_turns(self):
        port, fake = self._serve(CALL)
        base = {"model": "m", "tools": TOOLS}
        _post(port, dict(base, messages=[{"role": "user", "content": "one"}]))
        _post(port, dict(base, messages=[
            {"role": "user", "content": "one"},
            {"role": "assistant", "content": CALL},
            {"role": "user", "content": "two"}]))
        self.assertIn("<tools>", fake.prompts[0])
        self.assertNotIn("<tools>", fake.prompts[1])
        self.assertTrue(fake.prompts[1].startswith("<|im_end|>\n<|im_start|>"))


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -m unittest discover -s adapters/openai-tools/tests -t . -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'openai_tools.server'`

- [ ] **Step 3: Write minimal implementation**

`server.py` wires the pieces: a `ThreadingHTTPServer` with a handler that

1. parses the JSON body and rejects a missing `messages`,
2. keys a `Session` by the client's session identity (the `X-Session-Id` header when present, otherwise a hash of the first user message — documented in the README as a heuristic),
3. calls `session.next_delta(messages, tools)`,
4. sends the delta via the injected client's `infer`,
5. calls `session.record_generation(raw)`,
6. splits reasoning, parses tool calls, and assembles the OpenAI envelope with `finish_reason` `tool_calls` / `stop` / `length`,
7. maps any `BloomeryError` through `to_openai_error`.

The client is injected (constructor parameter) rather than constructed inside the handler — that is what lets the whole loop be tested with no daemon.

- [ ] **Step 4: Run test to verify it passes**

Run: `python3 -m unittest discover -s adapters/openai-tools/tests -t . -v`
Expected: all PASS

- [ ] **Step 5: Write the README**

`adapters/openai-tools/README.md` covers: what it is and what it deliberately does not do; how to re-extract the template and why it is committed; how to run it; the config keys; the `keep_reasoning_in_history` flag and the **unmeasured** deviation it controls (spec §6 amendment); and the two known gaps — agent accumulation (bloomery has no `DELETE /agents/{id}`) and the session-identity heuristic.

- [ ] **Step 6: Commit**

```bash
git add adapters/openai-tools/
git commit -m "feat: the OpenAI surface, wired end to end and testable with no GPU"
```

---

### Task 6: Live acceptance — HUMAN-GATED

**Do not start without Brice's explicit go.** This needs the GPU and a real hermes session.

**Files:**
- Create: `docs/superpowers/evidence/2026-XX-XX-openai-adapter-acceptance.md`

- [ ] **Step 1: Boot bloomery with the REAP-48 model**

Config as used on 2026-08-30: port 8498, `ctx_overhead_mib = 512`, `allow_unprofiled = true`, `assay.enabled = false`, model `qwen36-reap48-ours` → `/home/brice/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf`. Rebuild the featured binary **last** before booting: `cargo build -p bloomery-daemon --features vulkan`.

- [ ] **Step 2: Start the adapter against it and confirm a tool call end to end**

Record: the rendered preamble token count from bloomery's `prompt_tokens`, the parsed tool call, and the turn-2 `prompt_tokens` — which is the measurement that proves the preamble was not re-prefilled.

- [ ] **Step 3: Point a SCRATCH hermes profile at the adapter**

Never edit `~/.hermes/config.yaml` — it is a daily driver, running 34+ days. Use a scratch profile or a copied config directory.

- [ ] **Step 4: Record the honest result**

Report the tool-call parse rate over the session, the reset count, and any 413 the trajectory compressor had to absorb. **A poor parse rate is a finding, not a failure** — the model is an untrained, 48%-pruned MoE and spec §10.1 says so in advance.

- [ ] **Step 5: Commit the evidence and append to CARRIED-DEBT**

---

## Self-review

**Spec coverage.** §1 purpose → the whole plan. §2 model choice → Global Constraints and Task 6 Step 1. §3 wire format → Task 2. §4 components → one task per module. §5 data flow → Task 5. §6 session lifecycle *as amended* → Task 3, which cites the amendment. §7 error mapping → Task 4. §8 non-goals → the no-fabrication constraint is enforced by Task 2's negative tests and Task 5's `unparseable_output` test. §9 testing → each task carries its own tests; the GPU-free integration requirement is Task 5. §10 risks → risk 1 is Task 6 Step 4, risk 3 is Task 2's coercion tests, risk 4 is documented in Task 5's README step. §11 non-settled items are not implemented here, correctly.

**Placeholders.** None. Task 3 Step 3 deliberately ships an awkward expression with an implementer note telling them to write it cleanly from the tests — that is a stated instruction, not a gap.

**Type consistency.** `ChatTemplate.render_initial` / `render_turns` / `GENERATION_SUFFIX` are used identically in Tasks 1, 3 and 5. `Session.next_delta` returns `(delta, was_reset)` in both its definition and its use. `BloomeryClient.infer` returns a dict with `text` in Task 4's tests and Task 5's fake. `parse_tool_calls(visible, tools)` takes the same argument order everywhere.

**One gap I am flagging rather than hiding:** Task 5's session-identity heuristic (hash of the first user message when no `X-Session-Id` is supplied) is not specified in the design doc. It is an implementation detail with a real failure mode — two clients opening with identical first messages would share an agent. The README step documents it, and if it proves wrong the fix is a spec amendment, not a silent change.
