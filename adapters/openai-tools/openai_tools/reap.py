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

"""Idle-session reaping: the adapter owns agent turnover.

Why this exists (2026-09-07 dogfood, 25-tool set): every hermes conversation
left its bloomery agent resident forever. The adapter has no session-end
signal -- a `-q` client simply exits -- and only ever suspended an agent on
a history rewrite. On the daemon side `plan_residency` evicts only agents
of strictly LOWER priority (every adapter agent shares `default_priority`),
and the equal-priority time-share admits a requester only after IT has
waited a full quantum, which a one-shot client never does. So after as many
conversations as the tier holds windows, every further one was refused
with 409 until an operator deleted agents by hand.

Two reaping paths, both `DELETE /agents/{id}` (which frees the window;
`suspend` merely parks an agent and keeps its budget):

* **idle TTL** -- `reap_idle` deletes every session idle longer than
  `idle_ttl_secs`. `start_sweeper` runs it on a daemon thread.
* **residency** -- when an inference is refused with 409, the server
  calls `reap_one_idle` to delete the least-recently-used idle session and
  retries that inference once (`server._infer_and_respond`). A pure TTL
  cannot cover back-to-back conversations: any TTL long enough not to reap
  mid-conversation is too long to free a window for the next one.

"Idle" is decided by `entry.lock.acquire(blocking=False)`: a session with a
request in flight holds its lock for the whole request and is never a
candidate. A reaped entry is flagged `reaped` and dropped from the table
under `sessions_lock`; a request that had already fetched it re-fetches
(`_handle_chat_completion`) and gets a fresh agent plus a full render.

Every decision here is driven by `server.clock` (injected in tests), never
by wall time read directly, and nothing sleeps.
"""
import json
import logging
import threading

from .errors import BloomeryError

# Deliberately the server's logger, not `__name__`: the README promises one
# structured diagnostics stream on `openai_tools.server`, and a reap is a
# request-loop event, not a separate subsystem's.
logger = logging.getLogger("openai_tools.server")

DEFAULT_IDLE_TTL_SECS = 900
MAX_SWEEP_INTERVAL_SECS = 30


def idle_ttl(config: dict):
    """`idle_ttl_secs` from config; absent means the default, `null` disables."""
    return config.get("idle_ttl_secs", DEFAULT_IDLE_TTL_SECS)


def sweep_interval(ttl_secs: float) -> float:
    return min(ttl_secs / 2, MAX_SWEEP_INTERVAL_SECS)


def _snapshot(server) -> list:
    with server.sessions_lock:
        return list(server.sessions.items())


def _reap_locked(server, key: str, entry, reason: str, idle_secs: float) -> bool:
    """Delete `entry`'s agent and drop it from the table. The caller holds
    `entry.lock`, so no request can be using the agent while it goes.

    Best-effort in the same sense as `_suspend_best_effort`: a failed delete
    is logged, never raised, and the entry is KEPT so the next sweep tries
    again -- except a 404, which means the agent is already gone and the
    entry is stale bookkeeping to drop."""
    agent_id = entry.agent_id
    try:
        server.client.delete_agent(agent_id)
    except BloomeryError as exc:
        if exc.status != 404:
            logger.warning(json.dumps({"event": "delete_failed", "session": key,
                                       "agent": agent_id, "error": str(exc)}))
            return False
    except Exception as exc:  # TimeoutError / JSONDecodeError, as for suspend
        logger.warning(json.dumps({"event": "delete_failed", "session": key,
                                   "agent": agent_id, "error": str(exc)}))
        return False
    entry.reaped = True
    with server.sessions_lock:
        if server.sessions.get(key) is entry:
            del server.sessions[key]
    logger.info(json.dumps({"event": "session_reaped", "session": key, "agent": agent_id,
                            "reason": reason, "idle_secs": round(idle_secs, 3)}))
    return True


def reap_idle(server, now: float | None = None) -> list[str]:
    """Delete every session idle STRICTLY longer than the TTL. Returns the
    reaped session keys, in table order."""
    ttl = idle_ttl(server.config)
    if ttl is None:
        return []
    now = server.clock() if now is None else now
    reaped = []
    for key, entry in _snapshot(server):
        idle = now - entry.last_used
        if idle <= ttl:
            continue
        if not entry.lock.acquire(blocking=False):
            continue  # a request is in flight: not idle, whatever the clock says
        try:
            if entry.reaped:
                continue
            if _reap_locked(server, key, entry, "idle_ttl", idle):
                reaped.append(key)
        finally:
            entry.lock.release()
    return reaped


def reap_one_idle(server) -> str | None:
    """Delete the least-recently-used idle session. Returns its key, or None
    when nothing was idle (or every delete failed). The caller's own session
    is never a candidate: it holds its lock for the whole request, and the
    non-blocking acquire below is the only admission test."""
    now = server.clock()
    candidates = sorted(_snapshot(server), key=lambda item: (item[1].last_used, item[0]))
    for key, entry in candidates:
        if not entry.lock.acquire(blocking=False):
            continue
        try:
            if entry.reaped:
                continue
            if _reap_locked(server, key, entry, "residency", now - entry.last_used):
                return key
        finally:
            entry.lock.release()
    return None


def start_sweeper(server, interval_secs: float | None = None, on_sweep=None):
    """Run `reap_idle` every `interval_secs` (default `sweep_interval(ttl)`)
    on a daemon thread until `server.reap_stop` is set. Returns the thread,
    or None when the TTL is disabled. `on_sweep` is a test hook."""
    ttl = idle_ttl(server.config)
    if ttl is None:
        return None
    interval = sweep_interval(ttl) if interval_secs is None else interval_secs
    stop = server.reap_stop

    def run():
        while not stop.wait(interval):
            try:
                reap_idle(server)
            except Exception:
                logger.error("idle sweep failed", exc_info=True)
            if on_sweep is not None:
                on_sweep()

    thread = threading.Thread(target=run, name="openai-tools-reaper", daemon=True)
    thread.start()
    return thread
