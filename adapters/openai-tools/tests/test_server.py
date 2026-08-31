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

import json
import threading
import unittest
import urllib.error
import urllib.request

from openai_tools.errors import BloomeryError
from openai_tools.server import build_server

TPL = "adapters/openai-tools/templates/qwen36-reap48-ours.jinja"
TOOLS = [{"type": "function", "function": {
    "name": "terminal", "parameters": {"type": "object",
                                       "properties": {"command": {"type": "string"}}}}}]
CALL = ("\nthinking\n</think>\n\n<tool_call>\n<function=terminal>\n"
        "<parameter=command>\nls /tmp\n</parameter>\n</function>\n</tool_call>")

# Requirement A fixture: the `command` value literally contains the closing
# cascade `</parameter></function></tool_call>`. toolcall.py's scanner has no
# escaping mechanism, so that cascade closes the parameter, the function, and
# the tool_call all at once -- earlier than the model meant -- and everything
# after it is ordinary prose the parser never looks at again. It is this
# server's job not to lose that prose.
CASCADE = ("\nthinking\n</think>\n\n<tool_call>\n<function=terminal>\n"
           "<parameter=command>\nrm -rf /tmp</parameter></function></tool_call>\n"
           "note: that command clears the temp directory")

PLAIN = "\nreasoning\n</think>\n\nsure, here is the answer"

# A tool call with genuine natural-language prose BEFORE it (the template
# explicitly permits this: "You may provide optional reasoning for your
# function call in natural language BEFORE the function call"), so
# `content` for this reply is non-empty and checkable for leftover markup.
CALL_WITH_PREAMBLE = ("\nthinking\n</think>\n\nI'll check that for you.\n\n"
                      "<tool_call>\n<function=terminal>\n<parameter=command>\n"
                      "ls /tmp\n</parameter>\n</function>\n</tool_call>")


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


class _ResetTrackingBloomery(_FakeBloomery):
    """Critical 1: a distinct agent id per `create_agent` call (unlike
    `_FakeBloomery`'s constant `"a1"`), and records which agent id each
    `infer` call actually reached and which agent ids were `suspend`ed --
    so a test can tell whether a reset landed on a genuinely fresh agent
    and reclaimed the old one."""

    def __init__(self, reply):
        super().__init__(reply)
        self._next_id = 0
        self.infer_agent_ids = []
        self.suspended = []

    def create_agent(self, model, window_cap=None):
        self._next_id += 1
        return f"agent-{self._next_id}"

    def infer(self, agent_id, prompt, max_tokens):
        self.infer_agent_ids.append(agent_id)
        return super().infer(agent_id, prompt, max_tokens)

    def suspend(self, agent_id):
        self.suspended.append(agent_id)


class _ResetSuspendFailsBloomery(_ResetTrackingBloomery):
    """`suspend` raises a non-`BloomeryError` -- `TimeoutError`, the
    realistic case: a socket read timeout after headers, which `urllib`
    does not wrap in `URLError`, so `BloomeryClient._post` lets it through
    unchanged and it is not a `BloomeryError`. Used to prove
    `_suspend_best_effort`'s SUCCESS-path call (the old agent, mid-swap in
    `_infer_and_respond`) cannot revert an already-successful turn. The
    call is still recorded in `self.suspended` before raising, so a test
    can confirm suspend really was attempted."""

    def suspend(self, agent_id):
        self.suspended.append(agent_id)
        raise TimeoutError("suspend timed out")


class _OrderedCallBloomery(_FakeBloomery):
    """Fix round 1, Finding 2: records EVERY create_agent/suspend/infer
    call, in the exact order they happened, so a test can assert ORDER
    -- suspend of the outgoing agent before create of its replacement --
    not just that both eventually happened."""

    def __init__(self, reply):
        super().__init__(reply)
        self._next_id = 0
        self.calls = []

    def create_agent(self, model, window_cap=None):
        self._next_id += 1
        agent_id = f"agent-{self._next_id}"
        self.calls.append(("create", agent_id))
        return agent_id

    def infer(self, agent_id, prompt, max_tokens):
        self.calls.append(("infer", agent_id))
        return super().infer(agent_id, prompt, max_tokens)

    def suspend(self, agent_id):
        self.calls.append(("suspend", agent_id))


class _FailNBloomery(_FakeBloomery):
    """Critical 2: fails the Nth `infer` call(s) (1-indexed, `fail_calls` a
    set of attempt numbers) with a routine `BloomeryError` (a 413, exactly
    the designed-behaviour refusal path per spec §7), then behaves like
    `_FakeBloomery` otherwise. `all_prompts` records every attempted
    prompt, successful or not, so a test can compare a failed attempt's
    bytes against a later retry's."""

    def __init__(self, reply, fail_calls=frozenset()):
        super().__init__(reply)
        self.fail_calls = fail_calls
        self.attempt = 0
        self.all_prompts = []

    def infer(self, agent_id, prompt, max_tokens):
        self.attempt += 1
        self.all_prompts.append(prompt)
        if self.attempt in self.fail_calls:
            raise BloomeryError(413, {"error": "prompt_too_large",
                                      "needed_tokens": 999999, "window_tokens": 1})
        return super().infer(agent_id, prompt, max_tokens)


class _MaxTokensRecordingBloomery(_FakeBloomery):
    """Important 1 / Minor: records the `max_tokens` value that actually
    reached `infer`, so a test can check clamping and the
    `max_completion_tokens` alias without caring about the reply text."""

    def __init__(self, reply):
        super().__init__(reply)
        self.max_tokens_seen = []

    def infer(self, agent_id, prompt, max_tokens):
        self.max_tokens_seen.append(max_tokens)
        return super().infer(agent_id, prompt, max_tokens)


class _CrashingBloomery(_FakeBloomery):
    """Important 4: an unexpected (non-`BloomeryError`) exception from the
    client, standing in for a genuine bug rather than a routine daemon
    refusal -- must still become a generic 500, but with the traceback
    logged server-side."""

    def infer(self, agent_id, prompt, max_tokens):
        raise ValueError("unexpected-daemon-side-crash-sentinel")


def _post(port, payload, session_id=None):
    headers = {"Content-Type": "application/json"}
    if session_id is not None:
        headers["X-Session-Id"] = session_id
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/v1/chat/completions",
        data=json.dumps(payload).encode(),
        headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=10) as resp:
        return resp.status, json.loads(resp.read().decode())


def _post_sse(port, payload, session_id=None):
    headers = {"Content-Type": "application/json"}
    if session_id is not None:
        headers["X-Session-Id"] = session_id
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/v1/chat/completions",
        data=json.dumps(payload).encode(),
        headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=10) as resp:
        return resp.status, resp.headers.get("Content-Type"), resp.read().decode()


def _sse_data_lines(raw: str) -> list[str]:
    """Every `data: ...` payload in an SSE body, in order, as the raw text
    after the `data: ` prefix (still `[DONE]` for the terminal one -- a
    caller that needs JSON decodes every entry except that one itself, so
    a malformed non-DONE line fails the test loudly instead of being
    silently skipped). Asserts every event block is well-formed (`data: `
    prefix, blank-line terminated) rather than best-effort parsing it.
    """
    lines = []
    for block in raw.split("\n\n"):
        block = block.strip("\n")
        if not block:
            continue
        assert block.startswith("data: "), f"not a well-formed SSE data line: {block!r}"
        lines.append(block[len("data: "):])
    return lines


def _post_expect_error(port, payload, session_id=None):
    headers = {"Content-Type": "application/json"}
    if session_id is not None:
        headers["X-Session-Id"] = session_id
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/v1/chat/completions",
        data=json.dumps(payload).encode(),
        headers=headers, method="POST")
    try:
        urllib.request.urlopen(req, timeout=10)
        raise AssertionError("expected an HTTPError")
    except urllib.error.HTTPError as exc:
        with exc:
            raw = exc.read().decode()
        return exc.code, raw, json.loads(raw)


class ServerTest(unittest.TestCase):
    def _serve(self, reply, fake_cls=_FakeBloomery, cfg_extra=None):
        fake = fake_cls(reply)
        cfg = {"model": "m",
               "template": TPL,
               "max_tokens": 64}
        if cfg_extra:
            cfg.update(cfg_extra)
        srv = build_server(cfg, fake)
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        self.addCleanup(srv.shutdown)
        return srv.server_port, fake

    # --- brief's own Step 1 tests -----------------------------------

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
        # AUTHORISED CHANGE (Task 5, review round 1): originally hand-fed
        # {"role": "assistant", "content": CALL} (the raw model text) as
        # turn two -- which is exactly the shape record_generation used to
        # be limited to, and proved nothing about what a REAL client
        # actually echoes back (content + a separate tool_calls array).
        # Now feeds back the adapter's own returned `message` object,
        # making this a second genuine round trip rather than a
        # coincidental string match.
        port, fake = self._serve(CALL)
        base = {"model": "m", "tools": TOOLS}
        _, body1 = _post(port, dict(base, messages=[{"role": "user", "content": "one"}]))
        own_message = body1["choices"][0]["message"]
        _post(port, dict(base, messages=[
            {"role": "user", "content": "one"},
            own_message,
            {"role": "user", "content": "two"}]))
        self.assertIn("<tools>", fake.prompts[0])
        self.assertNotIn("<tools>", fake.prompts[1])
        self.assertTrue(fake.prompts[1].startswith("<|im_end|>\n<|im_start|>"))

    # --- Requirement A: visible text survives into content -----------

    def test_a_cascade_closed_tool_call_still_surfaces_trailing_text_as_content(self):
        port, _ = self._serve(CASCADE)
        status, body = _post(port, {"model": "m", "tools": TOOLS,
                                    "messages": [{"role": "user", "content": "clean up"}]})
        self.assertEqual(status, 200)
        choice = body["choices"][0]
        self.assertEqual(choice["finish_reason"], "tool_calls")
        self.assertEqual(choice["message"]["tool_calls"][0]["function"]["name"], "terminal")
        self.assertIn("note: that command clears the temp directory",
                      choice["message"]["content"])

    # --- Finding 1: content is client-visible text, never raw markup --

    def test_content_never_contains_reasoning_or_tool_call_markup_for_plain_text(self):
        port, _ = self._serve(PLAIN)
        status, body = _post(port, {"model": "m", "tools": TOOLS,
                                    "messages": [{"role": "user", "content": "hi"}]})
        content = body["choices"][0]["message"]["content"]
        self.assertEqual(content, "sure, here is the answer")
        self.assertNotIn("<think>", content)
        self.assertNotIn("</think>", content)
        self.assertNotIn("<tool_call>", content)

    def test_content_never_contains_reasoning_or_tool_call_markup_for_a_tool_call(self):
        port, _ = self._serve(CALL_WITH_PREAMBLE)
        status, body = _post(port, {"model": "m", "tools": TOOLS,
                                    "messages": [{"role": "user", "content": "ls"}]})
        choice = body["choices"][0]
        self.assertEqual(choice["finish_reason"], "tool_calls")
        content = choice["message"]["content"]
        self.assertEqual(content, "I'll check that for you.")
        self.assertNotIn("<think>", content)
        self.assertNotIn("</think>", content)
        self.assertNotIn("<tool_call>", content)
        self.assertNotIn("</tool_call>", content)

    # --- Requirement B: the KV benefit survives a REAL round trip ----

    def test_kv_benefit_survives_a_real_round_trip_for_plain_text(self):
        port, fake = self._serve(PLAIN)
        base = {"model": "m", "tools": TOOLS}
        _, body1 = _post(port, dict(base, messages=[{"role": "user", "content": "one"}]))
        own_message = body1["choices"][0]["message"]
        status2, _ = _post(port, dict(base, messages=[
            {"role": "user", "content": "one"},
            own_message,
            {"role": "user", "content": "two"}]))
        self.assertEqual(status2, 200)
        self.assertEqual(len(fake.prompts), 2)
        self.assertNotIn("<tools>", fake.prompts[1])

    def test_kv_benefit_survives_a_real_round_trip_for_a_tool_call(self):
        port, fake = self._serve(CALL)
        base = {"model": "m", "tools": TOOLS}
        _, body1 = _post(port, dict(base, messages=[{"role": "user", "content": "one"}]))
        own_message = body1["choices"][0]["message"]
        status2, _ = _post(port, dict(base, messages=[
            {"role": "user", "content": "one"},
            own_message,
            {"role": "user", "content": "two"}]))
        self.assertEqual(status2, 200)
        self.assertEqual(len(fake.prompts), 2)
        self.assertNotIn("<tools>", fake.prompts[1])

    # --- Requirement C: UnrenderableMessage becomes a 400 -------------

    def test_malformed_tool_call_arguments_return_400_not_500(self):
        port, fake = self._serve(CALL)
        secret_marker = "TOTALLY_NOT_VALID_JSON_SENTINEL_XYZ"
        broken = {"role": "assistant", "content": "",
                  "tool_calls": [{"id": "call_1", "type": "function",
                                  "function": {"name": "terminal",
                                               "arguments": secret_marker}}]}
        status, raw, body = _post_expect_error(port, {
            "model": "m", "tools": TOOLS,
            "messages": [{"role": "user", "content": "one"}, broken,
                         {"role": "user", "content": "two"}]})
        self.assertEqual(status, 400)
        self.assertEqual(body["error"]["type"], "invalid_request_error")
        self.assertNotIn(secret_marker, raw)
        self.assertEqual(fake.prompts, [])  # never reached bloomery at all

    # --- Finding 3: client-supplied tool_calls are never dropped -----

    def test_hybrid_content_and_tool_calls_history_preserves_the_tool_call(self):
        # A client-authored assistant turn with BOTH non-empty content AND
        # a real tool_calls array -- the exact shape an earlier
        # stripping heuristic would have silently discarded tool_calls
        # from. There is no such stripping any more: incoming history
        # reaches Session unmodified, so this renders via Session's own
        # native tool_calls handling (the function name shows up in the
        # rendered prompt) rather than vanishing.
        port, fake = self._serve(CALL)
        hybrid = {"role": "assistant", "content": "here's what I'll do",
                  "tool_calls": [{"id": "call_1", "type": "function",
                                  "function": {"name": "terminal",
                                               "arguments": json.dumps({"command": "ls"})}}]}
        status, body = _post(port, {
            "model": "m", "tools": TOOLS,
            "messages": [{"role": "user", "content": "one"}, hybrid,
                         {"role": "user", "content": "two"}]})
        self.assertEqual(status, 200)
        prompt = fake.prompts[0]
        self.assertIn("here's what I'll do", prompt)
        self.assertIn("<function=terminal>", prompt)

    # --- Also wired, per the brief -----------------------------------

    def test_get_v1_models_lists_the_configured_model(self):
        port, _ = self._serve(CALL)
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/v1/models", timeout=10) as resp:
            status = resp.status
            body = json.loads(resp.read().decode())
        self.assertEqual(status, 200)
        self.assertEqual(body["data"][0]["id"], "m")

    def test_missing_messages_is_a_400(self):
        port, _ = self._serve(CALL)
        status, _, body = _post_expect_error(port, {"model": "m"})
        self.assertEqual(status, 400)
        self.assertEqual(body["error"]["type"], "invalid_request_error")

    def test_finish_reason_is_length_when_generation_hits_the_token_cap(self):
        class _TruncatingBloomery(_FakeBloomery):
            def infer(self, agent_id, prompt, max_tokens):
                self.prompts.append(prompt)
                return {"text": self.reply, "prompt_tokens": 10,
                        "completion_tokens": max_tokens, "duration_ms": 3}

        port, _ = self._serve("\nhm\n</think>\n\nan answer that got cut off",
                              fake_cls=_TruncatingBloomery)
        status, body = _post(port, {"model": "m", "tools": TOOLS,
                                    "messages": [{"role": "user", "content": "go"}]})
        self.assertEqual(status, 200)
        self.assertEqual(body["choices"][0]["finish_reason"], "length")

    def test_bloomery_error_is_mapped_through_to_openai_error(self):
        class _RefusingBloomery(_FakeBloomery):
            def infer(self, agent_id, prompt, max_tokens):
                raise BloomeryError(413, {"error": "prompt_too_large",
                                          "needed_tokens": 900, "window_tokens": 512})

        port, _ = self._serve(CALL, fake_cls=_RefusingBloomery)
        status, _, body = _post_expect_error(port, {
            "model": "m", "tools": TOOLS,
            "messages": [{"role": "user", "content": "go"}]})
        self.assertEqual(status, 413)
        self.assertEqual(body["error"]["code"], "context_length_exceeded")

    def test_session_id_header_prevents_collision_on_identical_first_message(self):
        # Without an explicit header, session identity falls back to a hash
        # of the first user message (documented in the README as a
        # heuristic with a known limitation). Two DIFFERENT clients that
        # happen to open with the SAME first message must not be folded
        # into one bloomery agent when they each send their OWN
        # X-Session-Id.
        class _CountingBloomery(_FakeBloomery):
            def __init__(self, reply):
                super().__init__(reply)
                self.agents_created = 0

            def create_agent(self, model, window_cap=None):
                self.agents_created += 1
                return f"a{self.agents_created}"

        fake = _CountingBloomery(CALL)
        cfg = {"model": "m", "template": TPL, "max_tokens": 64}
        srv = build_server(cfg, fake)
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        self.addCleanup(srv.shutdown)

        def post_with_header(session_id):
            req = urllib.request.Request(
                f"http://127.0.0.1:{srv.server_port}/v1/chat/completions",
                data=json.dumps({"model": "m", "tools": TOOLS,
                                 "messages": [{"role": "user", "content": "hello"}]}).encode(),
                headers={"Content-Type": "application/json",
                        "X-Session-Id": session_id}, method="POST")
            with urllib.request.urlopen(req, timeout=10) as resp:
                return json.loads(resp.read().decode())

        post_with_header("client-a")
        post_with_header("client-b")
        self.assertEqual(fake.agents_created, 2)

    # --- Critical 1: a reset must land on a fresh agent -----------------

    def test_history_rewrite_sends_the_second_prompt_to_a_different_agent(self):
        port, fake = self._serve(PLAIN, fake_cls=_ResetTrackingBloomery)
        base = {"model": "m", "tools": TOOLS}
        _post(port, dict(base, messages=[{"role": "user", "content": "one"}]), "s1")
        # The client compressed history away: a real divergence, not an
        # append.
        _post(port, dict(base, messages=[{"role": "user", "content": "compressed"}]), "s1")
        self.assertEqual(len(fake.infer_agent_ids), 2)
        self.assertNotEqual(fake.infer_agent_ids[0], fake.infer_agent_ids[1])

    def test_history_rewrite_suspends_the_old_agent(self):
        port, fake = self._serve(PLAIN, fake_cls=_ResetTrackingBloomery)
        base = {"model": "m", "tools": TOOLS}
        _post(port, dict(base, messages=[{"role": "user", "content": "one"}]), "s1")
        old_agent = fake.infer_agent_ids[0]
        _post(port, dict(base, messages=[{"role": "user", "content": "compressed"}]), "s1")
        self.assertIn(old_agent, fake.suspended)

    def test_keep_reasoning_false_uses_a_fresh_agent_every_turn(self):
        # keep_reasoning=False is the spec's designated measurement CONTROL:
        # every turn is a full render (system + tools + entire history), so
        # every turn also needs a fresh, empty-KV agent -- not just resets.
        port, fake = self._serve(PLAIN, fake_cls=_ResetTrackingBloomery,
                                 cfg_extra={"keep_reasoning_in_history": False})
        base = {"model": "m", "tools": TOOLS}
        m1 = [{"role": "user", "content": "one"}]
        _post(port, dict(base, messages=m1), "s1")
        m2 = m1 + [{"role": "assistant", "content": "sure, here is the answer"},
                   {"role": "user", "content": "two"}]
        _post(port, dict(base, messages=m2), "s1")
        m3 = m2 + [{"role": "assistant", "content": "sure, here is the answer"},
                   {"role": "user", "content": "three"}]
        _post(port, dict(base, messages=m3), "s1")
        self.assertEqual(len(fake.infer_agent_ids), 3)
        self.assertEqual(len(set(fake.infer_agent_ids)), 3)
        for prompt in fake.prompts:
            self.assertIn("<tools>", prompt)   # every turn is a full render

    # --- Fix wave: a non-BloomeryError from suspend must not revert a ---
    # --- successful turn (_suspend_best_effort runs on the SUCCESS ------
    # --- path, before entry.agent_id/turns_committed are updated) -------

    def test_suspend_timeout_on_the_success_path_does_not_revert_the_commit(self):
        # _suspend_best_effort's call on the reset swap (suspending the
        # OLD agent) sits on the SUCCESS path in _infer_and_respond,
        # BEFORE entry.agent_id is reassigned to the new agent and BEFORE
        # record_generation. If suspend raises something that is not a
        # BloomeryError, it must still not be allowed to turn an infer
        # that already succeeded into a failed response, an orphaned new
        # agent, or a next turn silently appended onto the OLD agent's
        # stale KV.
        port, fake = self._serve(PLAIN, fake_cls=_ResetSuspendFailsBloomery)
        base = {"model": "m", "tools": TOOLS}
        status1, body1 = _post(
            port, dict(base, messages=[{"role": "user", "content": "one"}]), "s1")
        self.assertEqual(status1, 200)
        old_agent = fake.infer_agent_ids[0]

        # A history rewrite (genuine divergence) on turn 2 forces the
        # reset swap -- which is what makes _suspend_best_effort run on
        # the SUCCESS path for old_agent while a fresh agent is created.
        status2, body2 = _post(
            port, dict(base, messages=[{"role": "user", "content": "compressed"}]), "s1")

        # 1) the response is 200, not 500, even though suspend blew up.
        self.assertEqual(status2, 200)
        # 2) this turn's own content is correct and unaffected.
        self.assertEqual(body2["choices"][0]["message"]["content"],
                         "sure, here is the answer")
        self.assertIn(old_agent, fake.suspended)  # suspend really was attempted
        new_agent = fake.infer_agent_ids[1]
        self.assertNotEqual(old_agent, new_agent)

        # 3) session state afterwards is consistent: one more turn must be
        # an APPEND to the NEW agent (proving entry.agent_id really was
        # updated, i.e. the commit was not reverted) and not a fresh
        # full-render (proving turns_committed really was incremented
        # too) -- never routed back onto the old agent's stale KV.
        own_message = body2["choices"][0]["message"]
        status3, _ = _post(port, dict(base, messages=[
            {"role": "user", "content": "compressed"}, own_message,
            {"role": "user", "content": "three"}]), "s1")
        self.assertEqual(status3, 200)
        self.assertEqual(len(fake.infer_agent_ids), 3)
        self.assertEqual(fake.infer_agent_ids[2], new_agent)
        self.assertNotIn("<tools>", fake.prompts[2])

    # --- Important 3: a changed tool set is divergence too --------------

    def test_a_changed_tool_set_forces_a_reset_and_a_fresh_agent(self):
        port, fake = self._serve(PLAIN, fake_cls=_ResetTrackingBloomery)
        other_tools = [{"type": "function", "function": {
            "name": "other_tool", "parameters": {"type": "object", "properties": {}}}}]
        _, body1 = _post(port, {"model": "m", "tools": TOOLS,
                                "messages": [{"role": "user", "content": "one"}]}, "s1")
        own_message = body1["choices"][0]["message"]
        _post(port, {"model": "m", "tools": other_tools,
                     "messages": [{"role": "user", "content": "one"}, own_message,
                                 {"role": "user", "content": "two"}]}, "s1")
        self.assertEqual(len(fake.infer_agent_ids), 2)
        self.assertNotEqual(fake.infer_agent_ids[0], fake.infer_agent_ids[1])
        self.assertIn("<tools>", fake.prompts[1])

    # --- Critical 2: a failed infer must not corrupt session state ------

    def test_a_failed_first_turn_leaves_state_clean_for_an_identical_retry(self):
        fake = _FailNBloomery(PLAIN, fail_calls={1})
        cfg = {"model": "m", "template": TPL, "max_tokens": 64}
        srv = build_server(cfg, fake)
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        self.addCleanup(srv.shutdown)
        port = srv.server_port

        payload = {"model": "m", "tools": TOOLS,
                   "messages": [{"role": "user", "content": "one"}]}
        status1, _, _ = _post_expect_error(port, payload)
        self.assertEqual(status1, 413)
        status2, body2 = _post(port, payload)  # identical retry
        self.assertEqual(status2, 200)
        self.assertEqual(len(fake.all_prompts), 2)
        self.assertEqual(fake.all_prompts[0], fake.all_prompts[1])
        self.assertIn("<tools>", fake.all_prompts[1])
        self.assertIn("one", fake.all_prompts[1])

    def test_a_failed_mid_conversation_turn_leaves_state_clean_for_an_identical_retry(self):
        fake = _FailNBloomery(PLAIN, fail_calls={2})  # turn 1 succeeds, turn 2 fails
        cfg = {"model": "m", "template": TPL, "max_tokens": 64}
        srv = build_server(cfg, fake)
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        self.addCleanup(srv.shutdown)
        port = srv.server_port

        base = {"model": "m", "tools": TOOLS}
        _, body1 = _post(port, dict(base, messages=[{"role": "user", "content": "one"}]))
        own_message = body1["choices"][0]["message"]
        turn2 = dict(base, messages=[
            {"role": "user", "content": "one"}, own_message,
            {"role": "user", "content": "two"}])
        status2, _, _ = _post_expect_error(port, turn2)
        self.assertEqual(status2, 413)
        status3, _ = _post(port, turn2)  # identical retry
        self.assertEqual(status3, 200)
        # attempt 2 (failed) and attempt 3 (retry) must be byte-identical --
        # the previous bug produced a stunted 41-byte delta with the user's
        # question silently dropped instead.
        self.assertEqual(fake.all_prompts[1], fake.all_prompts[2])
        self.assertIn("two", fake.all_prompts[2])

    # --- Important 1 / Minor: max_tokens clamping and alias -------------

    def test_max_tokens_is_clamped_to_the_configured_cap(self):
        port, fake = self._serve(PLAIN, fake_cls=_MaxTokensRecordingBloomery,
                                 cfg_extra={"max_tokens_cap": 4096})
        _post(port, {"model": "m", "tools": TOOLS, "max_tokens": 128000,
                     "messages": [{"role": "user", "content": "hi"}]})
        self.assertEqual(fake.max_tokens_seen[-1], 4096)

    def test_max_tokens_under_the_cap_is_passed_through_unchanged(self):
        port, fake = self._serve(PLAIN, fake_cls=_MaxTokensRecordingBloomery,
                                 cfg_extra={"max_tokens_cap": 4096})
        _post(port, {"model": "m", "tools": TOOLS, "max_tokens": 100,
                     "messages": [{"role": "user", "content": "hi"}]})
        self.assertEqual(fake.max_tokens_seen[-1], 100)

    def test_max_completion_tokens_is_honoured_as_an_alias_for_max_tokens(self):
        port, fake = self._serve(PLAIN, fake_cls=_MaxTokensRecordingBloomery)
        _post(port, {"model": "m", "tools": TOOLS, "max_completion_tokens": 77,
                     "messages": [{"role": "user", "content": "hi"}]})
        self.assertEqual(fake.max_tokens_seen[-1], 77)

    # --- Important 4: diagnostics on an unexpected exception ------------

    def test_unexpected_exception_logs_a_traceback_and_returns_a_generic_500(self):
        port, _ = self._serve(CALL, fake_cls=_CrashingBloomery)
        with self.assertLogs("openai_tools.server", level="ERROR") as logs:
            status, raw, body = _post_expect_error(port, {
                "model": "m", "tools": TOOLS,
                "messages": [{"role": "user", "content": "hi"}]})
        self.assertEqual(status, 500)
        self.assertEqual(body["error"]["message"], "internal error")
        self.assertNotIn("unexpected-daemon-side-crash-sentinel", raw)
        self.assertTrue(
            any("unexpected-daemon-side-crash-sentinel" in entry for entry in logs.output))

    # --- Amendment 2026-08-31 "the retry state" --------------------------
    # From the live acceptance run: hermes sent a byte-identical request
    # twice ([system, user], then the same [system, user] again). After
    # the first succeeded, record_generation had appended the assistant
    # turn, so the retry -- shorter than the tracked list -- was
    # misclassified as a history rewrite: reset, fresh agent, and on a
    # tight tier a 409. This block reproduces that exact sequence and
    # pins the fix.

    def test_byte_identical_retry_replays_without_a_second_inference_or_new_agent(self):
        # Reproduces the captured hermes sequence. This is the RED this
        # task exists to turn GREEN: before the fix, this fails because
        # the retry is reset onto a second, different agent and re-runs
        # inference.
        port, fake = self._serve(PLAIN, fake_cls=_ResetTrackingBloomery)
        payload = {"model": "m", "tools": TOOLS,
                   "messages": [{"role": "system", "content": "sys"},
                               {"role": "user", "content": "one"}]}
        status1, body1 = _post(port, payload, "s1")
        status2, body2 = _post(port, payload, "s1")  # byte-identical retry
        self.assertEqual(status1, 200)
        self.assertEqual(status2, 200)
        self.assertEqual(body2, body1)                       # the SAME response
        self.assertEqual(len(fake.infer_agent_ids), 1)       # no second inference
        self.assertEqual(len(set(fake.infer_agent_ids)), 1)  # no new agent

    def test_a_second_identical_retry_falls_through_and_reinfers(self):
        # Bounded at one: after a single replay, a further identical
        # request must NOT replay again -- it falls through to the
        # ordinary path, is classified a rewrite by the existing rules
        # (the tracked list is longer than the retry), and is re-inferred
        # against a fresh agent. No second counting mechanism: this falls
        # out of next_delta's own append/rewrite rules once the replay
        # window has been used.
        port, fake = self._serve(PLAIN, fake_cls=_ResetTrackingBloomery)
        payload = {"model": "m", "tools": TOOLS,
                   "messages": [{"role": "system", "content": "sys"},
                               {"role": "user", "content": "one"}]}
        _post(port, payload, "s1")                      # turn 1
        _post(port, payload, "s1")                       # replay (bounded to one)
        status3, _ = _post(port, payload, "s1")           # second identical retry
        self.assertEqual(status3, 200)
        self.assertEqual(len(fake.infer_agent_ids), 2)    # a real second inference happened
        self.assertNotEqual(fake.infer_agent_ids[0], fake.infer_agent_ids[1])  # fresh agent

    def test_retry_replay_is_logged(self):
        port, fake = self._serve(PLAIN, fake_cls=_ResetTrackingBloomery)
        payload = {"model": "m", "tools": TOOLS,
                   "messages": [{"role": "user", "content": "one"}]}
        _post(port, payload, "s1")
        with self.assertLogs("openai_tools.server", level="INFO") as logs:
            _post(port, payload, "s1")
        self.assertTrue(any("retry_replayed" in entry for entry in logs.output))

    def test_a_genuine_append_is_unaffected_by_retry_handling(self):
        # A genuine follow-up turn (not a retry, not a rewrite) must still
        # append onto the SAME agent and must not be misread as a retry
        # replay of the prior response.
        port, fake = self._serve(PLAIN, fake_cls=_ResetTrackingBloomery)
        base = {"model": "m", "tools": TOOLS}
        _, body1 = _post(port, dict(base, messages=[{"role": "user", "content": "one"}]), "s1")
        own_message = body1["choices"][0]["message"]
        status2, body2 = _post(port, dict(base, messages=[
            {"role": "user", "content": "one"}, own_message,
            {"role": "user", "content": "two"}]), "s1")
        self.assertEqual(status2, 200)
        self.assertEqual(len(fake.infer_agent_ids), 2)
        self.assertEqual(len(set(fake.infer_agent_ids)), 1)   # same agent: append, not reset
        self.assertNotIn("<tools>", fake.prompts[1])
        self.assertNotEqual(body2, body1)

    def test_retry_is_recognised_after_a_multi_turn_conversation(self):
        # Not just turn 1: a retry of a later turn's request must also
        # replay rather than reset.
        port, fake = self._serve(PLAIN, fake_cls=_ResetTrackingBloomery)
        base = {"model": "m", "tools": TOOLS}
        m1 = [{"role": "user", "content": "one"}]
        _, body1 = _post(port, dict(base, messages=m1), "s1")
        own1 = body1["choices"][0]["message"]
        m2 = m1 + [own1, {"role": "user", "content": "two"}]
        _, body2 = _post(port, dict(base, messages=m2), "s1")
        status3, body3 = _post(port, dict(base, messages=m2), "s1")  # retry of turn 2
        self.assertEqual(status3, 200)
        self.assertEqual(body3, body2)
        self.assertEqual(len(fake.infer_agent_ids), 2)   # no third inference

    # --- Fix round 1, Finding 1: a changed tool set outranks the retry ---
    # --- rule -- a stale <tools> block must not be silently replayed  ----

    def test_a_retry_with_changed_tools_is_not_replayed_it_resets(self):
        # Live-verified gap: is_retry used to compare only messages, so a
        # retry with a DIFFERENT tools array was replayed verbatim,
        # silently hiding that the resident <tools> block is now stale.
        port, fake = self._serve(PLAIN, fake_cls=_ResetTrackingBloomery)
        other_tools = [{"type": "function", "function": {
            "name": "other_tool", "parameters": {"type": "object", "properties": {}}}}]
        messages = [{"role": "user", "content": "one"}]
        status1, body1 = _post(port, {"model": "m", "tools": TOOLS, "messages": messages}, "s1")
        status2, body2 = _post(
            port, {"model": "m", "tools": other_tools, "messages": messages}, "s1")
        self.assertEqual(status1, 200)
        self.assertEqual(status2, 200)
        self.assertNotEqual(body2, body1)                        # NOT a replay
        self.assertEqual(len(fake.infer_agent_ids), 2)           # a real second inference
        self.assertNotEqual(fake.infer_agent_ids[0], fake.infer_agent_ids[1])  # fresh agent
        self.assertIn("<tools>", fake.prompts[1])                # full render, new tools baked in

    def test_a_retry_with_identical_tools_still_replays(self):
        # The under-comparison half: an equal (by content) tools value
        # must not spuriously defeat a genuine retry.
        port, fake = self._serve(PLAIN, fake_cls=_ResetTrackingBloomery)
        messages = [{"role": "user", "content": "one"}]
        same_tools_new_list = list(TOOLS)
        status1, body1 = _post(port, {"model": "m", "tools": TOOLS, "messages": messages}, "s1")
        status2, body2 = _post(
            port, {"model": "m", "tools": same_tools_new_list, "messages": messages}, "s1")
        self.assertEqual(status1, 200)
        self.assertEqual(status2, 200)
        self.assertEqual(body2, body1)
        self.assertEqual(len(fake.infer_agent_ids), 1)

    # --- Fix round 1, Finding 2: suspend the outgoing agent BEFORE -------
    # --- creating the replacement, not after -----------------------------

    def test_a_reset_suspends_the_outgoing_agent_before_creating_the_replacement(self):
        # On this tier the pager's static budget places exactly one
        # context at a time (docs/superpowers/evidence/2026-08-31-openai-
        # adapter-acceptance.md §3). Creating the replacement BEFORE
        # suspending the outgoing agent makes both momentarily resident,
        # which the live acceptance run showed fails placement outright
        # (409). Order, not just eventual occurrence, is what matters.
        port, fake = self._serve(PLAIN, fake_cls=_OrderedCallBloomery)
        base = {"model": "m", "tools": TOOLS}
        _post(port, dict(base, messages=[{"role": "user", "content": "one"}]), "s1")
        old_agent = next(agent_id for kind, agent_id in fake.calls if kind == "create")
        fake.calls.clear()

        # A history rewrite forces the reset path.
        _post(port, dict(base, messages=[{"role": "user", "content": "compressed"}]), "s1")

        ordering = [call for call in fake.calls if call[0] in ("suspend", "create")]
        self.assertEqual(ordering[0], ("suspend", old_agent))
        self.assertEqual(ordering[1][0], "create")

    # --- Fix round 1, Finding 3 (ruling, ADR): the replay bound is one ---
    # --- replay PER INFERENCE, not per session -- sustained identical ----
    # --- retries alternate infer/replay rather than looping either way --

    def test_sustained_identical_retries_alternate_infer_and_replay(self):
        # Pins the ruling: five identical requests in a row must alternate
        # real inference and replay (infer, replay, infer, replay, infer)
        # -- never two replays in a row (which would starve a client that
        # genuinely could not use the answer) and never a reset loop.
        port, fake = self._serve(PLAIN, fake_cls=_ResetTrackingBloomery)
        payload = {"model": "m", "tools": TOOLS,
                   "messages": [{"role": "user", "content": "one"}]}
        responses = [_post(port, payload, "s1") for _ in range(5)]

        for status, _ in responses:
            self.assertEqual(status, 200)
        bodies = [body for _, body in responses]

        self.assertEqual(bodies[1], bodies[0])       # turn 2: replay of turn 1
        self.assertNotEqual(bodies[2], bodies[1])    # turn 3: bound spent, re-infers
        self.assertEqual(bodies[3], bodies[2])       # turn 4: replay of turn 3
        self.assertNotEqual(bodies[4], bodies[3])    # turn 5: bound spent, re-infers

        # Exactly three real inferences (turns 1, 3, 5), each a genuinely
        # distinct agent (turn 1's initial agent, then a reset per re-infer).
        self.assertEqual(len(fake.infer_agent_ids), 3)
        self.assertEqual(len(set(fake.infer_agent_ids)), 3)


    # --- Streaming (buffered SSE), the amended §8 -----------------------
    # From the live acceptance run: every real hermes chat request carries
    # `stream: true` (the `request_dump_*.json` fixtures used to write the
    # non-streaming tests above are error dumps, not a sample of normal
    # traffic -- see the spec's process note). Without SSE, hermes fails
    # before a session ever starts. This is buffered, not token-
    # incremental: the whole reply is generated before anything is
    # emitted, same limitation as bloomery's own `/v1` (`api_v1.rs`,
    # "Streaming (D3)").

    def test_streaming_request_gets_event_stream_content_type_and_terminal_done(self):
        port, _ = self._serve(PLAIN)
        status, content_type, raw = _post_sse(port, {
            "model": "m", "tools": TOOLS, "stream": True,
            "messages": [{"role": "user", "content": "hi"}]})
        self.assertEqual(status, 200)
        self.assertEqual(content_type, "text/event-stream")
        lines = _sse_data_lines(raw)
        self.assertEqual(lines[-1], "[DONE]")

    def test_every_streamed_chunk_is_valid_json_shaped_as_a_chat_completion_chunk(self):
        port, _ = self._serve(PLAIN)
        _, _, raw = _post_sse(port, {
            "model": "m", "tools": TOOLS, "stream": True,
            "messages": [{"role": "user", "content": "hi"}]})
        lines = _sse_data_lines(raw)
        for line in lines[:-1]:  # every line except the terminal [DONE]
            chunk = json.loads(line)  # raises loudly if this isn't valid JSON
            self.assertEqual(chunk["object"], "chat.completion.chunk")

    def test_a_streamed_text_reply_carries_content_in_a_delta_and_a_final_stop(self):
        port, _ = self._serve(PLAIN)
        _, _, raw = _post_sse(port, {
            "model": "m", "tools": TOOLS, "stream": True,
            "messages": [{"role": "user", "content": "hi"}]})
        chunks = [json.loads(line) for line in _sse_data_lines(raw)[:-1]]
        self.assertEqual(chunks[0]["choices"][0]["delta"]["content"],
                         "sure, here is the answer")
        self.assertEqual(chunks[0]["choices"][0]["delta"]["role"], "assistant")
        self.assertIsNone(chunks[0]["choices"][0]["finish_reason"])
        self.assertEqual(chunks[-1]["choices"][0]["finish_reason"], "stop")
        self.assertEqual(chunks[-1]["choices"][0]["delta"], {})

    def test_a_streamed_tool_call_carries_an_index_on_each_delta_tool_call(self):
        # The detail that most often breaks a client: omitting `index` is
        # the usual reason a client silently drops a streamed tool call.
        port, _ = self._serve(CALL)
        _, _, raw = _post_sse(port, {
            "model": "m", "tools": TOOLS, "stream": True,
            "messages": [{"role": "user", "content": "ls"}]})
        chunks = [json.loads(line) for line in _sse_data_lines(raw)[:-1]]
        call = chunks[0]["choices"][0]["delta"]["tool_calls"][0]
        self.assertEqual(call["index"], 0)
        self.assertEqual(call["type"], "function")
        self.assertIn("id", call)
        self.assertEqual(call["function"]["name"], "terminal")
        self.assertEqual(json.loads(call["function"]["arguments"]), {"command": "ls /tmp"})
        self.assertEqual(chunks[-1]["choices"][0]["finish_reason"], "tool_calls")

    def test_include_usage_produces_a_usage_chunk_with_empty_choices(self):
        port, _ = self._serve(PLAIN)
        _, _, raw = _post_sse(port, {
            "model": "m", "tools": TOOLS, "stream": True,
            "stream_options": {"include_usage": True},
            "messages": [{"role": "user", "content": "hi"}]})
        chunks = [json.loads(line) for line in _sse_data_lines(raw)[:-1]]
        usage_chunks = [c for c in chunks if "usage" in c]
        self.assertEqual(len(usage_chunks), 1)
        self.assertEqual(usage_chunks[0]["choices"], [])
        self.assertEqual(usage_chunks[0]["usage"]["prompt_tokens"], 10)
        self.assertEqual(usage_chunks[0]["usage"]["completion_tokens"], 5)

    def test_without_include_usage_no_usage_chunk_is_emitted(self):
        port, _ = self._serve(PLAIN)
        _, _, raw = _post_sse(port, {
            "model": "m", "tools": TOOLS, "stream": True,
            "messages": [{"role": "user", "content": "hi"}]})
        chunks = [json.loads(line) for line in _sse_data_lines(raw)[:-1]]
        self.assertFalse(any("usage" in c for c in chunks))

    def test_bloomery_error_on_a_streaming_request_stays_a_json_error_not_a_streamed_200(self):
        # The honest-refusal path (spec §7) must survive streaming: a
        # refusal that happens before any chunk is emitted stays an
        # ordinary JSON error with its real HTTP status. This is the
        # ordering consequence of buffering -- SSE headers are never
        # written until inference has actually succeeded.
        class _RefusingBloomery(_FakeBloomery):
            def infer(self, agent_id, prompt, max_tokens):
                raise BloomeryError(413, {"error": "prompt_too_large",
                                          "needed_tokens": 900, "window_tokens": 512})

        port, _ = self._serve(CALL, fake_cls=_RefusingBloomery)
        req = urllib.request.Request(
            f"http://127.0.0.1:{port}/v1/chat/completions",
            data=json.dumps({"model": "m", "tools": TOOLS, "stream": True,
                             "messages": [{"role": "user", "content": "go"}]}).encode(),
            headers={"Content-Type": "application/json"}, method="POST")
        try:
            urllib.request.urlopen(req, timeout=10)
            raise AssertionError("expected an HTTPError")
        except urllib.error.HTTPError as exc:
            self.assertEqual(exc.code, 413)
            self.assertEqual(exc.headers.get("Content-Type"), "application/json")
            with exc:
                body = json.loads(exc.read().decode())
        self.assertEqual(body["error"]["code"], "context_length_exceeded")

    def test_stream_false_explicit_returns_the_ordinary_json_response_unchanged(self):
        port, _ = self._serve(PLAIN)
        status, body = _post(port, {"model": "m", "tools": TOOLS, "stream": False,
                                    "messages": [{"role": "user", "content": "hi"}]})
        self.assertEqual(status, 200)
        self.assertEqual(body["object"], "chat.completion")

    def test_a_streamed_retry_replays_as_sse_with_no_second_inference(self):
        # Streaming is a presentation concern only -- the retry/append/
        # rewrite session logic underneath is unchanged, so a streaming
        # client's byte-identical retry still replays without a second
        # inference or a new agent, just rendered as SSE this time.
        port, fake = self._serve(PLAIN, fake_cls=_ResetTrackingBloomery)
        payload = {"model": "m", "tools": TOOLS, "stream": True,
                   "messages": [{"role": "user", "content": "one"}]}
        _post_sse(port, payload, "s1")
        status, content_type, raw = _post_sse(port, payload, "s1")
        self.assertEqual(status, 200)
        self.assertEqual(content_type, "text/event-stream")
        self.assertEqual(_sse_data_lines(raw)[-1], "[DONE]")
        self.assertEqual(len(fake.infer_agent_ids), 1)  # replay, no second inference


if __name__ == "__main__":
    unittest.main()
