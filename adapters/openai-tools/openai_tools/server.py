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

"""The HTTP surface: an OpenAI-compatible `/v1/chat/completions` and
`/v1/models`, wiring Tasks 1-4 (template, toolcall, session, bloomery
client) into something an OpenAI tool-calling client can talk to.

stdlib only (`http.server` / `urllib`) -- no new runtime dependency. The
bloomery client is INJECTED via `build_server(config, client)` rather than
constructed here, so the whole request/response loop is testable against a
scripted stub with no daemon and no GPU (see tests/test_server.py).

Two carried findings this file exists to close, both from earlier task
reviews on this branch:

  Requirement A (visible text must survive `tool_calls`): toolcall.py's
  scanner has no escaping mechanism, so a parameter value that happens to
  contain the literal cascade `</parameter></function></tool_call>` closes
  the block early; whatever the model meant to say next becomes ordinary
  prose that `parse_tool_calls` never inspects again. Satisfied here as a
  direct consequence of Requirement B's design (below): `content` is
  always the model's full, verbatim output, so nothing after an
  early-closed block is ever dropped.

  Requirement B (the KV benefit must survive a REAL round trip):
  `Session.record_generation` (session.py, CLOSED, not modified here)
  stores exactly `{"role": "assistant", "content": raw}` -- it has no
  parameter for `tool_calls`, and this is also literally what the brief's
  own `test_the_preamble_is_sent_once_across_two_turns` hand-feeds back on
  turn two (`{"role": "assistant", "content": CALL}`, `CALL` being the
  model's raw text). So the ONE representation `record_generation` can
  ever produce is content-only, holding the model's raw bytes -- and that
  is therefore the ONLY representation a later echo can be compared
  against and match.

  This handler honours that constraint on BOTH sides instead of fighting
  it: `message["content"]` in the response sent back to the client is
  always the model's raw, unprocessed output (`raw`), whether or not
  `tool_calls` also parsed -- never a stripped-down tail. That is what
  makes Requirement A trivial (the raw text contains everything, always).
  And before any incoming `messages` list is handed to `next_delta`,
  `_prepare_incoming_messages` drops a `tool_calls` field from any
  assistant turn that already carries non-empty `content` -- because, by
  this adapter's own convention, that content is already the complete,
  self-sufficient raw record of what happened, and `tool_calls` alongside
  it is redundant for anything Session needs to compare or render. A
  client faithfully echoing this server's own response therefore reduces,
  turn after turn, to exactly the content-only shape `record_generation`
  stored -- an honest match, not a coincidence -- so `_is_extension`
  recognises the real continuation and the `<tools>` preamble is never
  resent. (An assistant turn with tool_calls but EMPTY content -- the more
  common shape for an external client building history from scratch
  rather than round-tripping this adapter -- is left untouched, and
  Session's own native tool_calls handling, already covered by
  session.py's test suite, applies to it normally.)
"""
import hashlib
import json
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from .bloomery import BloomeryClient
from .errors import BloomeryError, to_openai_error
from .session import Session, UnrenderableMessage
from .template import ChatTemplate
from .toolcall import parse_tool_calls, split_reasoning


def _error_body(kind: str, code: str, message: str) -> dict:
    return {"error": {"type": kind, "code": code, "message": message}}


def _prepare_incoming_messages(messages):
    """A new list, with `tool_calls` dropped from any assistant message
    that already carries non-empty `content`.

    This adapter's own responses always set `content` to the model's raw,
    unprocessed output (see the module docstring's Requirement B section)
    -- so for a turn THIS adapter generated, `content` alone is already a
    complete, self-sufficient record, and a `tool_calls` field alongside
    it is redundant. Stripping it here, before the message ever reaches
    `Session`, is what lets a client's faithful echo of our own response
    be recognised as a plain content-only continuation -- exactly the
    shape `Session.record_generation` itself produces -- instead of
    forcing a reset every tool-using turn.

    An assistant message with tool_calls but EMPTY/absent content -- the
    ordinary shape for a client building history from scratch rather than
    round-tripping this adapter -- is left untouched, so `Session`'s own
    native tool_calls handling applies normally.

    Never mutates the caller's messages; builds new dicts throughout.
    """
    prepared = []
    for message in messages:
        if (isinstance(message, dict) and message.get("role") == "assistant"
                and message.get("tool_calls") and message.get("content")):
            message = {k: v for k, v in message.items() if k != "tool_calls"}
        prepared.append(message)
    return prepared


def _first_user_message_key(messages) -> str:
    """Session identity when the client sends no `X-Session-Id`: a SHA-256
    hash of the first user message. This is a heuristic, not a real
    identity -- see the README's "known limitations": two different
    clients that happen to open with the identical first user message
    collide onto the same bloomery agent. Callers that care should send
    `X-Session-Id` explicitly.
    """
    for message in messages:
        if message.get("role") == "user":
            content = message.get("content")
            material = content if isinstance(content, str) else json.dumps(content, sort_keys=True)
            return hashlib.sha256(material.encode("utf-8")).hexdigest()
    # No user turn at all (unusual, but not our call to reject): fall back
    # to hashing the whole history rather than crashing.
    return hashlib.sha256(json.dumps(messages, sort_keys=True).encode("utf-8")).hexdigest()


class _SessionEntry:
    """One bloomery agent, one `Session`, and a lock serializing the
    turns sent against it -- KV-append correctness requires requests for
    the same session never interleave."""

    def __init__(self, session: Session, agent_id: str):
        self.session = session
        self.agent_id = agent_id
        self.lock = threading.Lock()


class _Handler(BaseHTTPRequestHandler):
    # HTTP/1.0: each turn is dominated by inference latency, not
    # connection setup, so keep-alive buys nothing here and this avoids
    # lingering-socket ResourceWarnings under ThreadingHTTPServer.
    protocol_version = "HTTP/1.0"

    def log_message(self, format, *args):  # noqa: A002 - stdlib signature
        pass  # quiet by default; operational logging is the daemon's job

    def _send_json(self, status: int, body: dict) -> None:
        raw = json.dumps(body).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        if self.path == "/v1/models":
            model = self.server.config["model"]
            self._send_json(200, {
                "object": "list",
                "data": [{"id": model, "object": "model", "owned_by": "bloomery"}],
            })
            return
        self._send_json(404, _error_body("invalid_request_error", "not_found", "no such route"))

    def do_POST(self):
        if self.path != "/v1/chat/completions":
            self._send_json(404, _error_body("invalid_request_error", "not_found", "no such route"))
            return
        try:
            self._handle_chat_completion()
        except Exception:
            # Last-resort guard: never leak a bare traceback (or any
            # internal detail) to the client. Known failure modes are
            # already handled explicitly below; this is only a backstop.
            self._send_json(500, _error_body("server_error", "internal_error",
                                             "internal error"))

    def _read_json_body(self):
        length = int(self.headers.get("Content-Length") or 0)
        raw_body = self.rfile.read(length) if length else b""
        if not raw_body:
            return {}
        return json.loads(raw_body.decode("utf-8"))

    def _get_session(self, session_key: str, model: str) -> _SessionEntry:
        with self.server.sessions_lock:
            entry = self.server.sessions.get(session_key)
            if entry is None:
                agent_id = self.server.client.create_agent(
                    model, self.server.config.get("window_cap"))
                session = Session(
                    agent_id, self.server.template,
                    keep_reasoning=self.server.config.get("keep_reasoning_in_history", True))
                entry = _SessionEntry(session, agent_id)
                self.server.sessions[session_key] = entry
            return entry

    def _handle_chat_completion(self):
        try:
            payload = self._read_json_body()
        except json.JSONDecodeError:
            self._send_json(400, _error_body(
                "invalid_request_error", "invalid_json", "request body is not valid JSON"))
            return

        messages = payload.get("messages")
        if not isinstance(messages, list) or not messages:
            self._send_json(400, _error_body(
                "invalid_request_error", "missing_messages",
                "'messages' is required and must be a non-empty array"))
            return

        tools = payload.get("tools")
        model = payload.get("model") or self.server.config["model"]
        max_tokens = payload.get("max_tokens") or self.server.config.get("max_tokens", 512)

        session_key = self.headers.get("X-Session-Id") or _first_user_message_key(messages)
        entry = self._get_session(session_key, model)

        with entry.lock:
            self._infer_and_respond(entry, messages, tools, model, max_tokens)

    def _infer_and_respond(self, entry, messages, tools, model, max_tokens):
        session = entry.session
        messages = _prepare_incoming_messages(messages)

        try:
            delta, _was_reset = session.next_delta(messages, tools)
        except UnrenderableMessage as exc:
            # Requirement C: a malformed `arguments` string is a CLIENT
            # error under the OpenAI wire format (which defines
            # `arguments` as JSON) -- surfaced as 400, not left to escape
            # as an unhandled 500 that would blame the server for the
            # client's request. `str(exc)` never carries the raw
            # malformed payload (see UnrenderableMessage's own docstring
            # and session.py's test for it).
            self._send_json(400, _error_body(
                "invalid_request_error", "invalid_tool_arguments", str(exc)))
            return

        try:
            result = self.server.client.infer(entry.agent_id, delta, max_tokens)
        except BloomeryError as exc:
            status, body = to_openai_error(exc)
            self._send_json(status, body)
            return

        raw = result["text"]
        session.record_generation(raw)

        _reasoning, visible = split_reasoning(raw)
        calls = parse_tool_calls(visible, tools)

        # Requirement A + B: `content` is always the model's raw,
        # unprocessed output -- see the module docstring. This both
        # guarantees any trailing text after an early-closed tool_call
        # block is preserved (it is a substring of `raw`, always) and
        # keeps a round-tripped client echo byte-identical to what
        # `record_generation` just stored.
        if calls is not None:
            message = {"role": "assistant", "content": raw, "tool_calls": calls}
            finish_reason = "tool_calls"
        else:
            # Never fabricate: an unparsed generation is returned as plain
            # content, never as a synthesized tool_calls entry.
            message = {"role": "assistant", "content": raw}
            completion_tokens = result.get("completion_tokens")
            hit_cap = completion_tokens is not None and completion_tokens >= max_tokens
            finish_reason = "length" if hit_cap else "stop"

        self._send_json(200, self._envelope(model, message, finish_reason, result))

    @staticmethod
    def _envelope(model: str, message: dict, finish_reason: str, result: dict) -> dict:
        prompt_tokens = result.get("prompt_tokens", 0)
        completion_tokens = result.get("completion_tokens", 0)
        return {
            "id": f"chatcmpl-{uuid.uuid4().hex}",
            "object": "chat.completion",
            "created": int(time.time()),
            "model": model,
            "choices": [{
                "index": 0,
                "message": message,
                "finish_reason": finish_reason,
            }],
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": prompt_tokens + completion_tokens,
            },
        }


def build_server(config: dict, client) -> ThreadingHTTPServer:
    """Wire the four modules into a `ThreadingHTTPServer`.

    `client` is injected rather than constructed here -- a
    `BloomeryClient(base_url)` for real use, or a scripted stub in tests,
    so this whole loop is testable with no daemon and no GPU.
    """
    template = ChatTemplate.load(config["template"])
    port = config.get("port", 0)
    host = config.get("host", "127.0.0.1")
    server = ThreadingHTTPServer((host, port), _Handler)
    server.config = config
    server.client = client
    server.template = template
    server.sessions: dict[str, _SessionEntry] = {}
    server.sessions_lock = threading.Lock()
    return server


def main(argv=None) -> int:
    import sys

    argv = sys.argv[1:] if argv is None else argv
    config_path = argv[0] if argv else "adapters/openai-tools/config.json"
    with open(config_path, encoding="utf-8") as handle:
        config = json.load(handle)
    client = BloomeryClient(config["base_url"])
    server = build_server(config, client)
    print(f"openai-tools adapter listening on http://{server.server_address[0]}:"
          f"{server.server_port}")
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
