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
  prose that `parse_tool_calls` never inspects again. `_visible_content`
  (below) builds `content` from the FULL visible span with only the
  recognised `<tool_call>...</tool_call>` blocks removed, so that trailing
  (or leading, or interstitial) prose is always kept, never discarded
  because `tool_calls` is also present.

  Requirement B (the KV benefit must survive a REAL round trip): a
  content-only record can never carry `tool_calls`, so a caller that
  returns `{content, tool_calls}` to its own client had no way, via a
  plain string, to make what `Session` tracks match what that client will
  later echo back -- only `content` could ever line up, never
  `tool_calls`, and `Session._is_extension` requires both. Round 1 of this
  task's review authorised extending `Session.record_generation` (still
  the ONLY change to session.py; `_is_extension` itself is untouched) to
  also accept a full assistant message dict, stored via the same
  `_normalize_message` `next_delta` already uses for incoming history.
  This handler now passes `record_generation` the EXACT dict it is about
  to return to the client -- so the tracked record and a real client's
  echo of that object are identical by construction, not by coincidence,
  and no reset is forced on a genuine continuation.

  `content` itself is never the model's raw bytes: it is built by
  `_visible_content`, which strips the `<think>...</think>` reasoning
  block (via `toolcall.split_reasoning`) and, when tool calls parsed,
  removes the `<tool_call>...</tool_call>` XML too -- leaving only the
  natural-language text the template explicitly permits before a call.
  Client-visible text never contains reasoning scaffolding or tool-call
  markup. Incoming history is passed to `next_delta` completely
  unmodified -- there is no longer any stripping of a caller-supplied
  `tool_calls` field; `Session`'s own native tool_calls handling (already
  covered by session.py's test suite) applies to every incoming message
  exactly as written.
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


_TOOL_CALL_OPEN = "<tool_call>"
_TOOL_CALL_CLOSE = "</tool_call>"


def _visible_content(visible: str, call_count: int) -> str:
    """The client-visible `content` for a generation: `visible` (already
    reasoning-stripped by `toolcall.split_reasoning`) with its first
    `call_count` `<tool_call>...</tool_call>` spans removed.

    Found with the same left-to-right literal-substring search
    toolcall.py's own scanner uses, so the split points agree with what
    the parser actually consumed -- without needing toolcall.py to expose
    the spans it walked. `parse_tool_calls` reports names and arguments,
    not positions, and a successfully-parsed call can still leave real
    prose around it: the template explicitly permits natural-language
    reasoning BEFORE a call, and a parameter value containing the literal
    closing cascade `</parameter></function></tool_call>` can end a block
    early (no escaping in this grammar), leaving whatever the model meant
    to say next as ordinary text here rather than lost (Requirement A).

    When `call_count` is 0, this is just `visible` itself -- the plain-text
    reply case.
    """
    pieces = []
    pos = 0
    for _ in range(call_count):
        start = visible.find(_TOOL_CALL_OPEN, pos)
        pieces.append(visible[pos:start])
        end = visible.find(_TOOL_CALL_CLOSE, start)
        pos = end + len(_TOOL_CALL_CLOSE)
    pieces.append(visible[pos:])
    return "".join(pieces).strip()


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
        _reasoning, visible = split_reasoning(raw)
        calls = parse_tool_calls(visible, tools)

        # Requirement A: `content` is client-visible text only -- the
        # reasoning block is stripped by split_reasoning above, and (when
        # calls parsed) the <tool_call> XML is stripped by
        # _visible_content too, leaving just the natural-language prose
        # the template permits before a call. Any text after an
        # early-closed block (the cascade case) is still part of
        # `visible` and therefore still kept.
        if calls is not None:
            content = _visible_content(visible, len(calls)) or None
            message = {"role": "assistant", "content": content, "tool_calls": calls}
            finish_reason = "tool_calls"
        else:
            # Never fabricate: an unparsed generation is returned as plain
            # content, never as a synthesized tool_calls entry.
            message = {"role": "assistant", "content": visible}
            completion_tokens = result.get("completion_tokens")
            hit_cap = completion_tokens is not None and completion_tokens >= max_tokens
            finish_reason = "length" if hit_cap else "stop"

        # Requirement B: record_generation gets the EXACT dict about to be
        # returned to the client -- so what Session tracks and what a
        # real client echoes back next turn are identical by
        # construction, not by coincidence (session.py's authorised
        # extension for this task; _is_extension itself is untouched).
        session.record_generation(message)

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
