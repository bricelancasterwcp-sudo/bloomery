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
import logging
import sys
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from .bloomery import BloomeryClient
from .errors import BloomeryError, to_openai_error
from .session import Session, UnrenderableMessage
from .template import ChatTemplate
from .toolcall import parse_tool_calls, split_reasoning

# Fix wave, Important 4: minimal structured diagnostics to stderr. Task 6's
# live acceptance run (real daemon, no test harness) has never been
# attempted, and before this the only failure signal was a bare 500 with no
# server-side trace of what happened. One handler, attached once, dependency
# -free (stdlib `logging`); `log_message` below stays quiet -- this is a
# purpose-built request log, not the raw HTTP access log.
logger = logging.getLogger(__name__)
if not logger.handlers:
    _handler = logging.StreamHandler(sys.stderr)
    _handler.setFormatter(logging.Formatter("%(message)s"))
    logger.addHandler(_handler)
    logger.setLevel(logging.INFO)


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
        # Critical 1: whether ANY turn has ever been successfully committed
        # to this entry's (current or a prior) agent. Only used to tell "the
        # first turn of a session" (agent is already fresh, just created)
        # apart from every later turn -- never touched by a failed attempt,
        # so a failed-then-retried first turn is still correctly treated as
        # the first turn.
        self.turns_committed = 0
        # Amendment 2026-08-31 "the retry state": the exact response
        # object last returned for this session, and whether it has
        # already been replayed once. Both are updated ONLY on a
        # successful real turn (never on a failed attempt, and never by
        # the replay path itself) -- see _infer_and_respond. Bounding
        # replay at one is this single flag, reset every time a real turn
        # actually commits; no second counting mechanism.
        self.last_response = None
        self.replayed_once = False


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
            # Important 4: the traceback still goes to stderr (exc_info=True
            # on the logging call, not the response body) so an unexpected
            # failure during Task 6's live acceptance run is diagnosable
            # server-side, without ever leaking internals to the client.
            logger.error("unhandled exception handling POST %s", self.path, exc_info=True)
            self._send_json(500, _error_body("server_error", "internal_error",
                                             "internal error"))

    def _read_json_body(self):
        length = int(self.headers.get("Content-Length") or 0)
        raw_body = self.rfile.read(length) if length else b""
        if not raw_body:
            return {}
        return json.loads(raw_body.decode("utf-8"))

    def _get_session(self, session_key: str, model: str) -> _SessionEntry:
        # Minor fix: `create_agent` is a network call to the daemon: it must
        # not run while holding `sessions_lock`, or one session's first
        # request blocks every OTHER session's `_get_session` for the full
        # round trip. Double-checked: read under the lock, create outside
        # it, then re-check under the lock before publishing -- if another
        # thread's request for the SAME session_key won the race, use its
        # entry and suspend the redundant agent we created rather than
        # leaking it.
        with self.server.sessions_lock:
            entry = self.server.sessions.get(session_key)
        if entry is not None:
            return entry

        agent_id = self.server.client.create_agent(
            model, self.server.config.get("window_cap"))
        session = Session(
            agent_id, self.server.template,
            keep_reasoning=self.server.config.get("keep_reasoning_in_history", True))
        candidate = _SessionEntry(session, agent_id)

        with self.server.sessions_lock:
            entry = self.server.sessions.get(session_key)
            if entry is None:
                self.server.sessions[session_key] = candidate
                return candidate

        # Lost the race: another thread already published an entry for this
        # session_key. Our agent was never used for anything; abandon it.
        self._suspend_best_effort(agent_id)
        return entry

    def _suspend_best_effort(self, agent_id: str) -> None:
        try:
            self.server.client.suspend(agent_id)
        except Exception as exc:
            # VRAM-hygiene cleanup, not correctness for THIS request; do not
            # let a suspend failure block or fail the response it rides
            # along with. Widened from `except BloomeryError` (too narrow):
            # `BloomeryClient._post` can also raise `json.JSONDecodeError`
            # (a 200 with a non-JSON body) or `TimeoutError` (a socket read
            # timeout after headers -- urllib does not wrap that in
            # `URLError`), neither of which is a `BloomeryError`. This call
            # runs on the SUCCESS path in `_infer_and_respond`, before
            # `entry.agent_id`/`turns_committed` are updated and before
            # `record_generation` -- an escape here would revert an
            # already-successful turn's commit. Logged so a swallow here is
            # never silent.
            logger.warning(json.dumps({"event": "suspend_failed", "agent": agent_id,
                                       "error": str(exc)}))

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
        max_tokens = self._resolve_max_tokens(payload)

        session_key = self.headers.get("X-Session-Id") or _first_user_message_key(messages)
        entry = self._get_session(session_key, model)

        with entry.lock:
            self._infer_and_respond(entry, session_key, messages, tools, model, max_tokens)

    def _resolve_max_tokens(self, payload: dict) -> int:
        """Minor fix: `max_completion_tokens` is honoured as an alias for
        `max_tokens` (the newer OpenAI field name; some clients send one,
        some the other). Important 1: the resolved value is then clamped to
        `config["max_tokens_cap"]` (default 4096) -- real hermes requests
        carry 64000/65536/128000, which unclamped against a ~98k-103k-token
        window makes the daemon's window law 413 nearly every request,
        which then hits Critical 2's failure mode. A smaller client-
        supplied value is still honoured verbatim; only the ceiling is
        enforced.
        """
        requested = payload.get("max_tokens")
        if requested is None:
            requested = payload.get("max_completion_tokens")
        if requested is None:
            requested = self.server.config.get("max_tokens", 512)
        cap = self.server.config.get("max_tokens_cap", 4096)
        return min(requested, cap)

    def _send_invalid_tool_arguments(self, session_key, agent_id, exc) -> None:
        # Requirement C: a malformed `arguments` string is a CLIENT error
        # under the OpenAI wire format (which defines `arguments` as
        # JSON) -- surfaced as 400, not left to escape as an unhandled 500
        # that would blame the server for the client's request. `str(exc)`
        # never carries the raw malformed payload (see UnrenderableMessage's
        # own docstring and session.py's test for it). Shared by both
        # `session.is_retry` and `session.next_delta`'s raise paths --
        # neither mutates session state before raising, so no restore is
        # needed either way.
        self._log_event(session_key, agent_id, None, None, "invalid_tool_arguments")
        self._send_json(400, _error_body(
            "invalid_request_error", "invalid_tool_arguments", str(exc)))

    def _infer_and_respond(self, entry, session_key, messages, tools, model, max_tokens):
        session = entry.session

        # Amendment 2026-08-31 "the retry state", checked BEFORE
        # next_delta per the spec: session.is_retry reuses
        # _is_extension's own per-message comparison rather than a
        # second, looser one, so retry detection and next_delta's
        # append/rewrite classification can never disagree. A replay
        # touches neither the KV nor the agent -- no session mutation, no
        # bloomery call. Bounded at one by entry.replayed_once, which is
        # cleared again only when a REAL turn actually commits (below);
        # a second identical request therefore falls through to the
        # ordinary path unchanged, where next_delta classifies it a
        # rewrite (the tracked list is longer) and it is re-inferred
        # against a fresh agent -- no second counting mechanism.
        try:
            retry = session.is_retry(messages)
        except UnrenderableMessage as exc:
            self._send_invalid_tool_arguments(session_key, entry.agent_id, exc)
            return

        if retry and not entry.replayed_once:
            entry.replayed_once = True
            self._log_event(session_key, entry.agent_id, False, None, "retry_replayed")
            self._send_json(200, entry.last_response)
            return

        # Critical 2: snapshot BEFORE next_delta -- next_delta itself still
        # mutates immediately (every existing session.py test relies on
        # that), so undoing a downstream delivery failure is this method's
        # job via `session.restore`, not next_delta's.
        snapshot = session.snapshot()

        try:
            delta, was_reset = session.next_delta(messages, tools)
        except UnrenderableMessage as exc:
            # next_delta itself never mutates before raising, so no
            # restore is needed here.
            self._send_invalid_tool_arguments(session_key, entry.agent_id, exc)
            return

        # Critical 1: a delta from the full-render path must land on an
        # agent with an EMPTY KV -- bloomery appends, so sending a full
        # render (system + tools + entire history) onto an agent whose KV
        # already holds a prior conversation duplicates it. This is needed
        # not only when `was_reset` is True (the client diverged) but also
        # when `keep_reasoning=False` chose not to reuse an otherwise-
        # legitimate append (spec's measurement control): both take the
        # full-render path, and `entry.turns_committed == 0` is the one
        # case where the CURRENT agent is already guaranteed fresh (it was
        # just created for this session's first turn), so no swap is
        # needed there.
        needs_fresh_agent = (
            entry.turns_committed > 0 and (was_reset or not session.keep_reasoning))

        send_agent_id = entry.agent_id
        created_agent_id = None

        try:
            if needs_fresh_agent:
                created_agent_id = self.server.client.create_agent(
                    model, self.server.config.get("window_cap"))
                send_agent_id = created_agent_id
            result = self.server.client.infer(send_agent_id, delta, max_tokens)
        except BloomeryError as exc:
            # No session state may change until the daemon has accepted the
            # bytes: restore undoes next_delta's mutation, so an honest
            # retry of the identical request reproduces the identical
            # delta, never a stunted one built on the false belief this
            # attempt's bytes already landed in the KV.
            session.restore(snapshot)
            if created_agent_id is not None:
                # Never used for anything (infer to it failed); abandon it
                # rather than leaking it. The OLD agent (entry.agent_id) is
                # untouched -- it was never actually replaced.
                self._suspend_best_effort(created_agent_id)
            self._log_event(session_key, send_agent_id, was_reset, len(delta),
                            f"bloomery_error_{exc.status}")
            status, body = to_openai_error(exc)
            self._send_json(status, body)
            return

        # Success: the daemon has accepted the bytes. Only now does the
        # agent swap (if any) actually take effect.
        if created_agent_id is not None:
            self._suspend_best_effort(entry.agent_id)
            entry.agent_id = created_agent_id
        entry.turns_committed += 1

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

        # Amendment 2026-08-31 "the retry state": cache the EXACT response
        # object just returned, and clear replayed_once -- this turn just
        # became the new committed state, so a retry of THIS turn earns
        # its own fresh replay allowance. Only reached on success, same as
        # entry.turns_committed above.
        body = self._envelope(model, message, finish_reason, result)
        entry.last_response = body
        entry.replayed_once = False

        self._log_event(session_key, entry.agent_id, was_reset, len(delta), "ok",
                        prompt_tokens=result.get("prompt_tokens"),
                        completion_tokens=result.get("completion_tokens"))
        self._send_json(200, body)

    def _log_event(self, session_key, agent_id, was_reset, delta_bytes, outcome,
                   prompt_tokens=None, completion_tokens=None) -> None:
        """Important 4: one structured line to stderr per request -- the
        only diagnostic signal available the first time this adapter is run
        against a real daemon (Task 6's live acceptance run). Deliberately
        plain JSON on a single `logger.info` call rather than a new
        dependency."""
        logger.info(json.dumps({
            "session": session_key,
            "agent": agent_id,
            "reset": was_reset,
            "delta_bytes": delta_bytes,
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "outcome": outcome,
        }))

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
