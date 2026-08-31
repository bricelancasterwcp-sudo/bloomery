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


def _post_expect_error(port, payload):
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/v1/chat/completions",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"}, method="POST")
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


if __name__ == "__main__":
    unittest.main()
