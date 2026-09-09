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

"""Idle-session reaping (2026-09-07 dogfood finding).

The 25-tool dogfood (`docs/superpowers/evidence/2026-09-07-hermes-dogfood-25-tools.md`)
showed that every hermes conversation leaves its bloomery agent resident
forever: the adapter has no session-end signal, `plan_residency` never
evicts an equal-priority agent, and the pager's time-share needs the
requester to wait a quantum that a one-shot client never does. After as
many conversations as the tier holds windows, every further one is
refused with 409.

Two reaping paths, both adapter-side, both `DELETE /agents/{id}`:
  * idle TTL -- a sweep deletes sessions idle longer than `idle_ttl_secs`;
  * residency -- a 409 on infer deletes the least-recently-used idle
    session and retries that inference once.
Every test here drives the logic with an injected clock and no thread.
"""
import importlib.util
import json
import logging
import os
import sys
import tempfile
import threading
import unittest
from http.server import ThreadingHTTPServer

from openai_tools import reap
from openai_tools.errors import BloomeryError
from openai_tools.server import build_server
from tests.test_server import CALL, PLAIN, TOOLS, TPL, _post

_REFUSAL = {"error": "refused", "needed": 1217134592, "free": 545745056, "reclaimable": 0}


class _Clock:
    def __init__(self, now=1000.0):
        self.now = now

    def __call__(self):
        return self.now


class _TwoWindowBloomery:
    """A faithful model of the dogfood tier: distinct agent ids, and at most
    `windows` agents may be resident at once. An infer against a
    non-resident agent when the tier is full raises the daemon's 409
    exactly as bloomery does (residency is decided at infer, not create --
    the dogfood journal shows `AgentCreated a3` then `Refusal`). `delete_agent`
    frees the window; `suspend` deliberately does NOT (a parked agent keeps
    its budget, which is why suspend never solved the leak)."""

    def __init__(self, reply, windows=2):
        self.reply = reply
        self.windows = windows
        self._next_id = 0
        self.resident = []
        self.deleted = []
        self.suspended = []
        self.infer_agent_ids = []
        self.prompts = []

    def create_agent(self, model, window_cap=None):
        self._next_id += 1
        return f"agent-{self._next_id}"

    def infer(self, agent_id, prompt, max_tokens):
        self.infer_agent_ids.append(agent_id)
        if agent_id not in self.resident:
            if len(self.resident) >= self.windows:
                raise BloomeryError(409, dict(_REFUSAL))
            self.resident.append(agent_id)
        self.prompts.append(prompt)
        return {"text": self.reply, "prompt_tokens": 10, "completion_tokens": 5,
                "duration_ms": 3}

    def suspend(self, agent_id):
        self.suspended.append(agent_id)

    def delete_agent(self, agent_id):
        self.deleted.append(agent_id)
        if agent_id in self.resident:
            self.resident.remove(agent_id)


class _DeleteFailsBloomery(_TwoWindowBloomery):
    def __init__(self, reply, status=None):
        super().__init__(reply, windows=99)
        self.status = status

    def delete_agent(self, agent_id):
        self.deleted.append(agent_id)
        if self.status is None:
            raise TimeoutError("delete timed out")
        raise BloomeryError(self.status, {"error": "unknown_agent", "agent": agent_id})


def _user(text):
    return [{"role": "user", "content": text}]


class ReapTest(unittest.TestCase):
    def _serve(self, fake, cfg_extra=None, clock=None):
        cfg = {"model": "m", "template": TPL, "max_tokens": 64}
        if cfg_extra:
            cfg.update(cfg_extra)
        clock = clock or _Clock()
        srv = build_server(cfg, fake, clock=clock)
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        self.addCleanup(srv.shutdown)
        return srv, clock

    @staticmethod
    def _settle(srv):
        # `_post` returns the instant the response lands; the server thread
        # may still hold the entry lock for a few microseconds after that.
        # Taking and releasing every lock waits for exactly that, so a reap
        # that follows sees settled sessions -- no sleeps, no polling.
        for entry in list(srv.sessions.values()):
            with entry.lock:
                pass

    def _capture_log(self):
        records = []

        class _H(logging.Handler):
            def emit(self, record):
                records.append(record.getMessage())

        handler = _H()
        logger = logging.getLogger("openai_tools.server")
        logger.addHandler(handler)
        self.addCleanup(logger.removeHandler, handler)
        return records

    # --- bookkeeping -------------------------------------------------

    def test_last_used_is_stamped_at_creation_and_advanced_by_each_request(self):
        srv, clock = self._serve(_TwoWindowBloomery(PLAIN))
        clock.now = 1000.0
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        entry = srv.sessions["s1"]
        self.assertEqual(entry.last_used, 1000.0)
        clock.now = 1042.0
        _post(srv.server_port, {"messages": _user("one") + [
            {"role": "assistant", "content": "sure, here is the answer"},
            {"role": "user", "content": "two"}]}, session_id="s1")
        self.assertEqual(entry.last_used, 1042.0)

    # --- idle TTL ----------------------------------------------------

    def test_sessions_idle_past_the_ttl_are_deleted_and_dropped(self):
        fake = _TwoWindowBloomery(PLAIN)
        records = self._capture_log()
        srv, clock = self._serve(fake, {"idle_ttl_secs": 60})
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        clock.now += 10
        _post(srv.server_port, {"messages": _user("two")}, session_id="s2")
        clock.now += 61  # s1 idle 71 s, s2 idle 61 s: both past 60
        self._settle(srv)
        reaped = reap.reap_idle(srv)
        self.assertEqual(sorted(reaped), ["s1", "s2"])
        self.assertEqual(sorted(fake.deleted), ["agent-1", "agent-2"])
        self.assertEqual(srv.sessions, {})
        events = [json.loads(r) for r in records if '"session_reaped"' in r]
        self.assertEqual({e["reason"] for e in events}, {"idle_ttl"})
        self.assertEqual({e["agent"] for e in events}, {"agent-1", "agent-2"})

    def test_a_session_used_within_the_ttl_is_kept(self):
        fake = _TwoWindowBloomery(PLAIN)
        srv, clock = self._serve(fake, {"idle_ttl_secs": 60})
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        clock.now += 30
        _post(srv.server_port, {"messages": _user("two")}, session_id="s2")
        clock.now += 31  # s1 idle 61 s (past), s2 idle 31 s (kept)
        self._settle(srv)
        self.assertEqual(reap.reap_idle(srv), ["s1"])
        self.assertEqual(fake.deleted, ["agent-1"])
        self.assertEqual(list(srv.sessions), ["s2"])

    def test_the_ttl_boundary_is_strict(self):
        # Exactly `ttl` seconds idle is NOT past the ttl.
        fake = _TwoWindowBloomery(PLAIN)
        srv, clock = self._serve(fake, {"idle_ttl_secs": 60})
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        clock.now += 60
        self._settle(srv)
        self.assertEqual(reap.reap_idle(srv), [])
        clock.now += 0.5
        self.assertEqual(reap.reap_idle(srv), ["s1"])

    def test_a_session_mid_request_is_never_reaped(self):
        fake = _TwoWindowBloomery(PLAIN)
        srv, clock = self._serve(fake, {"idle_ttl_secs": 60})
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        entry = srv.sessions["s1"]
        clock.now += 1000
        with entry.lock:  # a request is in flight on s1
            self.assertEqual(reap.reap_idle(srv), [])
        self.assertEqual(fake.deleted, [])
        self.assertIn("s1", srv.sessions)
        self.assertEqual(reap.reap_idle(srv), ["s1"])  # released: now reapable

    def test_no_ttl_means_the_sweep_reaps_nothing(self):
        fake = _TwoWindowBloomery(PLAIN)
        srv, clock = self._serve(fake, {"idle_ttl_secs": None})
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        # Infinite idle time: any finite substitute for "disabled" (a huge
        # ttl, say) would reap here; only a genuine off-switch does not.
        clock.now = float("inf")
        self._settle(srv)
        self.assertEqual(reap.reap_idle(srv), [])
        self.assertEqual(fake.deleted, [])

    def test_a_reaped_session_that_returns_gets_a_fresh_agent_and_a_full_render(self):
        fake = _TwoWindowBloomery(PLAIN)
        srv, clock = self._serve(fake, {"idle_ttl_secs": 60})
        _post(srv.server_port, {"tools": TOOLS, "messages": _user("one")}, session_id="s1")
        clock.now += 61
        self._settle(srv)
        self.assertEqual(reap.reap_idle(srv), ["s1"])
        status, body = _post(srv.server_port, {"tools": TOOLS, "messages": _user("one") + [
            {"role": "assistant", "content": "sure, here is the answer"},
            {"role": "user", "content": "two"}]}, session_id="s1")
        self.assertEqual(status, 200)
        self.assertEqual(fake.infer_agent_ids, ["agent-1", "agent-2"])
        # The second prompt is a FULL render onto the fresh agent -- the
        # tools preamble is present again, not just the delta.
        self.assertIn("<tools>", fake.prompts[1])
        self.assertIn("two", fake.prompts[1])

    def test_a_request_that_reaches_a_reaped_entry_refetches_a_fresh_one(self):
        # The race the `reaped` flag exists for: a request fetched its entry,
        # then blocked on the lock while the reaper (holding that lock)
        # deleted the agent and dropped the entry. Reconstructed
        # deterministically: the test holds the lock, starts the request,
        # performs the reaper's exact effect under the lock, releases.
        fake = _TwoWindowBloomery(PLAIN)
        srv, clock = self._serve(fake, {"idle_ttl_secs": 60})
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        entry = srv.sessions["s1"]
        entered = threading.Event()
        inner = entry.lock

        class _SignalLock:
            # Same surface the handler and the reaper use; `entered` fires
            # the moment the worker has FETCHED the entry and is about to
            # block on the lock the test holds -- no sleeps, no guessing.
            def __enter__(self):
                entered.set()
                return inner.__enter__()

            def __exit__(self, *exc):
                return inner.__exit__(*exc)

            def acquire(self, blocking=True):
                return inner.acquire(blocking)

            def release(self):
                inner.release()

        entry.lock = _SignalLock()
        result = {}

        def request():
            result["resp"] = _post(srv.server_port, {"messages": _user("one") + [
                {"role": "assistant", "content": "sure, here is the answer"},
                {"role": "user", "content": "two"}]}, session_id="s1")

        inner.acquire()
        try:
            worker = threading.Thread(target=request, daemon=True)
            worker.start()
            self.assertTrue(entered.wait(5.0))
            clock.now += 61
            # The reaper's effect, verbatim (it holds the lock we hold):
            fake.delete_agent(entry.agent_id)
            entry.reaped = True
            with srv.sessions_lock:
                del srv.sessions["s1"]
        finally:
            inner.release()
        worker.join(10.0)
        status, _ = result["resp"]
        self.assertEqual(status, 200)
        # The request did NOT run on the deleted agent-1: it re-fetched and
        # got agent-2, and rendered the full conversation onto it.
        self.assertEqual(fake.infer_agent_ids, ["agent-1", "agent-2"])
        self.assertIn("two", fake.prompts[1])
        self.assertIs(srv.sessions["s1"].reaped, False)

    # --- residency ---------------------------------------------------

    def test_a_409_reaps_the_lru_idle_session_and_retries_once(self):
        fake = _TwoWindowBloomery(CALL, windows=2)
        records = self._capture_log()
        srv, clock = self._serve(fake, {"idle_ttl_secs": 900})
        _post(srv.server_port, {"tools": TOOLS, "messages": _user("one")}, session_id="s1")
        clock.now += 5
        _post(srv.server_port, {"tools": TOOLS, "messages": _user("two")}, session_id="s2")
        clock.now += 5
        # Third conversation: the dogfood's run 3. Two windows are full.
        status, body = _post(srv.server_port, {"tools": TOOLS, "messages": _user("three")},
                             session_id="s3")
        self.assertEqual(status, 200)
        self.assertEqual(body["choices"][0]["finish_reason"], "tool_calls")
        self.assertEqual(fake.deleted, ["agent-1"])          # LRU, not s2
        self.assertEqual(fake.resident, ["agent-2", "agent-3"])
        self.assertNotIn("s1", srv.sessions)
        self.assertIn("s2", srv.sessions)
        # infer was attempted, refused, then retried on the SAME agent.
        self.assertEqual(fake.infer_agent_ids, ["agent-1", "agent-2", "agent-3", "agent-3"])
        events = [json.loads(r) for r in records if '"session_reaped"' in r]
        self.assertEqual([(e["reason"], e["agent"]) for e in events], [("residency", "agent-1")])
        # The refused attempt is logged honestly, then the ok.
        outcomes = [json.loads(r)["outcome"] for r in records
                    if '"outcome"' in r and '"s3"' in r]
        self.assertEqual(outcomes, ["bloomery_error_409", "ok"])

    def test_lru_means_least_recently_used_not_oldest_created(self):
        fake = _TwoWindowBloomery(PLAIN, windows=2)
        srv, clock = self._serve(fake)
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        clock.now += 5
        _post(srv.server_port, {"messages": _user("two")}, session_id="s2")
        clock.now += 5
        # s1 is used again: now s2 is the LRU.
        _post(srv.server_port, {"messages": _user("one") + [
            {"role": "assistant", "content": "sure, here is the answer"},
            {"role": "user", "content": "more"}]}, session_id="s1")
        clock.now += 5
        status, _ = _post(srv.server_port, {"messages": _user("three")}, session_id="s3")
        self.assertEqual(status, 200)
        self.assertEqual(fake.deleted, ["agent-2"])

    def test_a_409_with_nothing_idle_reaches_the_client_unchanged(self):
        fake = _TwoWindowBloomery(PLAIN, windows=2)
        srv, clock = self._serve(fake)
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        _post(srv.server_port, {"messages": _user("two")}, session_id="s2")
        e1, e2 = srv.sessions["s1"], srv.sessions["s2"]
        with e1.lock, e2.lock:  # both mid-request: nothing may be reaped
            with self.assertRaises(Exception) as caught:
                _post(srv.server_port, {"messages": _user("three")}, session_id="s3")
        self.assertEqual(caught.exception.code, 409)
        body = json.loads(caught.exception.read().decode())
        self.assertEqual(body["error"]["code"], "residency_refused")
        self.assertIn("reclaimable 0 B", body["error"]["message"])
        self.assertEqual(fake.deleted, [])

    def test_the_residency_retry_is_bounded_at_one(self):
        # A tier that refuses forever: one reap, one retry, then the 409
        # goes to the client. Never a loop, never a second victim.
        fake = _TwoWindowBloomery(PLAIN, windows=2)
        srv, clock = self._serve(fake)
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        _post(srv.server_port, {"messages": _user("two")}, session_id="s2")
        fake.windows = 0  # from now on nothing new is ever resident
        with self.assertRaises(Exception) as caught:
            _post(srv.server_port, {"messages": _user("three")}, session_id="s3")
        self.assertEqual(caught.exception.code, 409)
        self.assertEqual(fake.deleted, ["agent-1"])
        self.assertEqual(fake.infer_agent_ids[-2:], ["agent-3", "agent-3"])

    def test_the_refused_session_itself_is_never_the_victim(self):
        fake = _TwoWindowBloomery(PLAIN, windows=1)
        srv, clock = self._serve(fake)
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        self._settle(srv)
        clock.now += 5
        status, _ = _post(srv.server_port, {"messages": _user("two")}, session_id="s2")
        self.assertEqual(status, 200)
        self.assertEqual(fake.deleted, ["agent-1"])
        self.assertIn("s2", srv.sessions)

    # --- delete failures ---------------------------------------------

    def test_a_failed_delete_is_logged_and_the_entry_kept_for_the_next_sweep(self):
        fake = _DeleteFailsBloomery(PLAIN)
        records = self._capture_log()
        srv, clock = self._serve(fake, {"idle_ttl_secs": 60})
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        clock.now += 61
        self._settle(srv)
        self.assertEqual(reap.reap_idle(srv), [])
        self.assertIn("s1", srv.sessions)
        self.assertEqual(fake.deleted, ["agent-1"])  # it was attempted
        failed = [json.loads(r) for r in records if '"delete_failed"' in r]
        self.assertEqual(len(failed), 1)
        self.assertEqual(failed[0]["agent"], "agent-1")

    def test_a_404_on_delete_means_already_gone_and_drops_the_entry(self):
        fake = _DeleteFailsBloomery(PLAIN, status=404)
        srv, clock = self._serve(fake, {"idle_ttl_secs": 60})
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        clock.now += 61
        self._settle(srv)
        self.assertEqual(reap.reap_idle(srv), ["s1"])
        self.assertNotIn("s1", srv.sessions)

    # --- sweeper wiring ----------------------------------------------

    def test_start_sweeper_is_disabled_by_a_null_ttl(self):
        srv, _ = self._serve(_TwoWindowBloomery(PLAIN), {"idle_ttl_secs": None})
        self.assertIsNone(reap.start_sweeper(srv))

    def test_start_sweeper_runs_a_daemon_thread_that_reaps(self):
        fake = _TwoWindowBloomery(PLAIN)
        srv, clock = self._serve(fake, {"idle_ttl_secs": 60})
        _post(srv.server_port, {"messages": _user("one")}, session_id="s1")
        clock.now += 61
        self._settle(srv)
        swept = threading.Event()
        thread = reap.start_sweeper(srv, interval_secs=0.01, on_sweep=swept.set)
        self.addCleanup(srv.reap_stop.set)
        self.assertTrue(thread.daemon)
        self.assertTrue(swept.wait(5.0))
        self.assertEqual(fake.deleted, ["agent-1"])

    def test_sweep_interval_is_half_the_ttl_capped_at_thirty_seconds(self):
        self.assertEqual(reap.sweep_interval(10), 5)
        self.assertEqual(reap.sweep_interval(900), 30)
        self.assertEqual(reap.sweep_interval(60), 30)


class EntrypointTest(unittest.TestCase):
    def test_under_python_dash_m_the_reap_events_share_the_request_logger(self):
        # Reproduces `python -m openai_tools.server` in-process: the module
        # is executed under the name `__main__` (with `__package__` set, as
        # `-m` does), so its `if __name__ == "__main__"` runs `main()`.
        # `serve_forever` is stubbed so `main()` returns instead of blocking.
        # The 2026-09-07 follow-up run measured ZERO `session_reaped` lines
        # while the daemon's journal showed the DELETE: `logger` was named
        # from `__name__`, i.e. `__main__`, and `reap.py`'s events went to a
        # handler-less `openai_tools.server`.
        with tempfile.TemporaryDirectory() as tmp:
            cfg = os.path.join(tmp, "config.json")
            with open(cfg, "w", encoding="utf-8") as handle:
                json.dump({"base_url": "http://127.0.0.1:1", "model": "m", "template": TPL,
                           "port": 0, "idle_ttl_secs": None}, handle)
            path = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                                "openai_tools", "server.py")
            spec = importlib.util.spec_from_file_location("__main__", path)
            module = importlib.util.module_from_spec(spec)
            module.__package__ = "openai_tools"
            saved_argv, saved_serve = sys.argv, ThreadingHTTPServer.serve_forever
            sys.argv = ["openai_tools.server", cfg]
            ThreadingHTTPServer.serve_forever = lambda self, *a, **k: None
            try:
                with self.assertRaises(SystemExit) as exited:
                    spec.loader.exec_module(module)
            finally:
                sys.argv, ThreadingHTTPServer.serve_forever = saved_argv, saved_serve
            self.assertEqual(exited.exception.code, 0)
        self.assertEqual(module.__name__, "__main__")
        self.assertIs(module.logger, reap.logger)
        self.assertEqual(module.logger.name, "openai_tools.server")
        self.assertTrue(module.logger.handlers, "the request logger must own a handler")
