"""Tests for `tools.memory_battery.driver.run_arm` (task-3 brief; design
spec §5), run against a scripted stdlib `http.server` fake standing in for
the daemon -- no GPU, no real daemon, no real sleeps.

`Script`/`ScriptedServer` below are the fake: `Script` is a per-PATH FIFO
queue of `(method, response)` pairs (keyed by path, not by global receipt
order) plus an optional "sticky" entry per path that keeps answering the
same way forever once its queue empties -- needed for the poll-deadline
test, where the exact number of polls before wall-clock trips is not
knowable in advance. `ScriptedServer` also records every request it
receives, in receipt order, for assertions that need to see the driver's
actual call shape (e.g. "zero task requests were made").

Every test drives `run_arm` end to end (its only produced interface, per
the brief) rather than reaching into private helpers -- matching
`test_corpus_check.py`'s own convention of exercising the public entry
point and reading its externally-visible effects (the ledger file, the
fake server's recorded calls, the on-disk workspace bytes).
"""

from __future__ import annotations

import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any

from tools.memory_battery.driver import IdentityMismatchError, run_arm

# ---------------------------------------------------------------------------
# Scripted fake daemon
# ---------------------------------------------------------------------------


class Script:
    """See module docstring. `add` queues one response for one exact path
    (consumed once, in registration order, on the next request to that
    path); `add_sticky` registers a response that keeps answering after its
    own path's queue is empty."""

    def __init__(self) -> None:
        self._queues: dict[str, list[tuple[str, Any]]] = {}
        self._sticky: dict[str, tuple[str, Any]] = {}

    def add(self, method: str, path: str, response: tuple[int, Any]) -> "Script":
        self._queues.setdefault(path, []).append((method, response))
        return self

    def add_sticky(self, method: str, path: str, response: tuple[int, Any]) -> "Script":
        self._sticky[path] = (method, response)
        return self

    def resolve(self, method: str, path: str) -> tuple[int, Any]:
        queue = self._queues.get(path)
        if queue:
            expected_method, response = queue.pop(0)
        elif path in self._sticky:
            expected_method, response = self._sticky[path]
        else:
            return 500, {"error": "unscripted_request", "method": method, "path": path}
        if method != expected_method:
            return 500, {
                "error": "method_mismatch",
                "path": path,
                "expected": expected_method,
                "got": method,
            }
        return response


class ScriptedServer:
    """A real `http.server.HTTPServer` on a background thread, single-
    threaded (never `ThreadingHTTPServer`) -- the driver only ever issues
    one HTTP request at a time, so strict one-at-a-time handling is both
    sufficient and what lets `Script`'s per-path FIFO ordering mean
    anything."""

    def __init__(self, script: Script) -> None:
        self.calls: list[tuple[str, str, Any]] = []
        calls = self.calls

        class Handler(BaseHTTPRequestHandler):
            def _handle(self, method: str) -> None:
                length = int(self.headers.get("Content-Length", 0) or 0)
                raw = self.rfile.read(length) if length else b""
                body = json.loads(raw) if raw else None
                calls.append((method, self.path, body))
                status, payload = script.resolve(method, self.path)
                data = json.dumps(payload).encode("utf-8") if payload is not None else b""
                self.send_response(status)
                if data:
                    self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                if data:
                    self.wfile.write(data)

            def do_GET(self) -> None:  # noqa: N802 -- http.server's own naming
                self._handle("GET")

            def do_POST(self) -> None:  # noqa: N802
                self._handle("POST")

            def log_message(self, *_args: Any) -> None:  # silence default stderr logging
                pass

        self._httpd = HTTPServer(("127.0.0.1", 0), Handler)
        self._thread = threading.Thread(target=self._httpd.serve_forever, daemon=True)
        self._thread.start()

    @property
    def base_url(self) -> str:
        return f"http://127.0.0.1:{self._httpd.server_port}"

    def close(self) -> None:
        self._httpd.shutdown()
        self._httpd.server_close()
        self._thread.join(timeout=5)


# ---------------------------------------------------------------------------
# Script-building helpers (one call shape each, matching the daemon's real
# wire contract -- design spec §5 / slice-1 acceptance: POST /agents ->
# 201 {id}; POST /agents/{id}/task -> 202 {task_id}; GET
# /agents/{id}/task/{task_id} -> 200 {status, steps, summary}; POST
# /agents/{id}/suspend -> 204 no body; GET /status -> {..., models: [...]}).
# ---------------------------------------------------------------------------


def _add_identity(script: Script, digest: str | None) -> None:
    script.add("GET", "/status", (200, {"models": [{"name": "m", "digest": digest, "loaded": True}]}))


def _add_create(script: Script, agent_id: str, http_status: int = 201) -> None:
    body = {"id": agent_id} if http_status == 201 else {"error": "boom"}
    script.add("POST", "/agents", (http_status, body))


def _add_submit(script: Script, agent_id: str, task_id: str, http_status: int = 202) -> None:
    body = {"task_id": task_id} if http_status == 202 else {"error": "boom"}
    script.add("POST", f"/agents/{agent_id}/task", (http_status, body))


def _add_poll(
    script: Script,
    agent_id: str,
    task_id: str,
    poll_status: str,
    http_status: int = 200,
    sticky: bool = False,
) -> None:
    path = f"/agents/{agent_id}/task/{task_id}"
    body = {"status": poll_status, "steps": [], "summary": None} if http_status == 200 else {"error": "boom"}
    if sticky:
        script.add_sticky("GET", path, (http_status, body))
    else:
        script.add("GET", path, (http_status, body))


def _add_suspend(script: Script, agent_id: str) -> None:
    script.add("POST", f"/agents/{agent_id}/suspend", (204, None))


def _add_ok_task(script: Script, agent_id: str, task_id: str, terminal: str = "Done") -> None:
    _add_create(script, agent_id)
    _add_submit(script, agent_id, task_id)
    _add_poll(script, agent_id, task_id, terminal)
    _add_suspend(script, agent_id)


# ---------------------------------------------------------------------------
# Manifest fixture (the fields `run_arm` actually reads: "tasks"[]."name",
# "goal", "grant".{read_roots,write_roots,commands} -- corpus.py's own
# per-task shape, task-1 report)
# ---------------------------------------------------------------------------


def _build_manifest(root: Path, names: list[str]) -> dict[str, Any]:
    tasks = []
    for name in names:
        task_dir = root / "tasks" / name
        (task_dir / "workspace").mkdir(parents=True)
        (task_dir / "pristine").mkdir(parents=True)
        (task_dir / "workspace" / "x.txt").write_text("pristine-bytes\n", encoding="utf-8")
        (task_dir / "pristine" / "x.txt").write_text("pristine-bytes\n", encoding="utf-8")
        workspace_abs = str((task_dir / "workspace").resolve())
        tasks.append(
            {
                "name": name,
                "family": "fam",
                "workspace": f"tasks/{name}/workspace",
                "goal": f"goal for {name}",
                "grant": {
                    "read_roots": [workspace_abs],
                    "write_roots": [workspace_abs],
                    "commands": [["python3", "-m", "unittest"]],
                },
            }
        )
    return {
        "instrument": "memory-battery-v1",
        "corpus_seed": 1,
        "n": len(names),
        "families": {"fam": len(names)},
        "tasks": tasks,
    }


def _read_ledger(path: Path) -> list[dict[str, Any]]:
    lines = path.read_text(encoding="utf-8").splitlines()
    return [json.loads(line) for line in lines if line.strip()]


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


class DriverInvariantsTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tmp_path = Path(self._tmp.name)

    def test_manifest_order_preserved_both_phases(self) -> None:
        names = ["t0", "t1", "t2"]
        manifest = _build_manifest(self.tmp_path, names)
        script = Script()
        _add_identity(script, "digest-x")
        for name in names:
            _add_ok_task(script, f"a1-{name}", f"tk1-{name}")
        _add_identity(script, "digest-x")
        for name in names:
            _add_ok_task(script, f"a2-{name}", f"tk2-{name}")

        server = ScriptedServer(script)
        self.addCleanup(server.close)
        ledger_path = self.tmp_path / "ledger.jsonl"

        run_arm(
            manifest, server.base_url, "C", "digest-x", ledger_path, poll_interval_s=0.0, task_deadline_s=5.0
        )

        rows = _read_ledger(ledger_path)
        phase1_tasks = [r["task"] for r in rows if r.get("event") != "identity" and r["phase"] == 1]
        phase2_tasks = [r["task"] for r in rows if r.get("event") != "identity" and r["phase"] == 2]
        self.assertEqual(phase1_tasks, names)
        self.assertEqual(phase2_tasks, names)

        # Same order is visible on the wire too: submit-task goals, in
        # receipt order, are phase1's names followed by phase2's names.
        submit_goals = [
            call[2]["goal"] for call in server.calls if call[0] == "POST" and call[1].endswith("/task")
        ]
        self.assertEqual(submit_goals, [f"goal for {n}" for n in names] * 2)

    def test_suspend_called_after_every_task_including_failures(self) -> None:
        names = ["fail_task", "ok_task"]
        manifest = _build_manifest(self.tmp_path, names)
        script = Script()
        _add_identity(script, "d1")
        _add_ok_task(script, "a1", "tk1", terminal="Error")
        _add_ok_task(script, "a2", "tk2", terminal="Done")
        _add_identity(script, "d1")
        _add_ok_task(script, "a3", "tk3", terminal="Error")
        _add_ok_task(script, "a4", "tk4", terminal="Done")

        server = ScriptedServer(script)
        self.addCleanup(server.close)
        ledger_path = self.tmp_path / "ledger.jsonl"

        run_arm(manifest, server.base_url, "C", "d1", ledger_path, poll_interval_s=0.0, task_deadline_s=5.0)

        suspend_calls = [c for c in server.calls if c[0] == "POST" and c[1].endswith("/suspend")]
        self.assertEqual(len(suspend_calls), 4)  # 2 tasks x 2 phases, incl. the "Error" ones
        suspended_agents = {c[1].split("/")[2] for c in suspend_calls}
        self.assertEqual(suspended_agents, {"a1", "a2", "a3", "a4"})

        rows = _read_ledger(ledger_path)
        statuses = {r["task"]: r["status"] for r in rows if r.get("event") != "identity" and r["phase"] == 1}
        self.assertEqual(statuses, {"fail_task": "Error", "ok_task": "Done"})

    def test_identity_mismatch_aborts_before_any_task_request(self) -> None:
        names = ["t0"]
        manifest = _build_manifest(self.tmp_path, names)
        script = Script()
        _add_identity(script, "wrong-digest")

        server = ScriptedServer(script)
        self.addCleanup(server.close)
        ledger_path = self.tmp_path / "ledger.jsonl"

        with self.assertRaises(IdentityMismatchError):
            run_arm(
                manifest,
                server.base_url,
                "C",
                "expected-digest",
                ledger_path,
                poll_interval_s=0.0,
                task_deadline_s=5.0,
            )

        task_related_calls = [c for c in server.calls if c[1] == "/agents" or "/task" in c[1]]
        self.assertEqual(task_related_calls, [], "no task request may occur once identity fails to match")

        rows = _read_ledger(ledger_path)
        identity_rows = [r for r in rows if r.get("event") == "identity"]
        self.assertEqual(len(identity_rows), 1)
        self.assertEqual(identity_rows[0]["digest"], "wrong-digest")
        self.assertEqual(identity_rows[0]["phase"], 1)

    def test_non_running_status_ends_polling(self) -> None:
        names = ["t0"]
        manifest = _build_manifest(self.tmp_path, names)
        script = Script()
        _add_identity(script, "d1")
        _add_create(script, "a1")
        _add_submit(script, "a1", "tk1")
        _add_poll(script, "a1", "tk1", "Running")
        _add_poll(script, "a1", "tk1", "Done")
        _add_suspend(script, "a1")
        _add_identity(script, "d1")
        _add_ok_task(script, "a2", "tk2", terminal="Done")

        server = ScriptedServer(script)
        self.addCleanup(server.close)
        ledger_path = self.tmp_path / "ledger.jsonl"

        run_arm(manifest, server.base_url, "C", "d1", ledger_path, poll_interval_s=0.0, task_deadline_s=5.0)

        poll_calls = [c for c in server.calls if c[0] == "GET" and c[1] == "/agents/a1/task/tk1"]
        self.assertEqual(len(poll_calls), 2, "polling must stop the instant a non-Running status is seen")

    def test_scripted_500_records_driver_infra_and_continues(self) -> None:
        names = ["boom_task", "next_task"]
        manifest = _build_manifest(self.tmp_path, names)
        script = Script()
        _add_identity(script, "d1")
        _add_create(script, "a1")
        _add_submit(script, "a1", "tk1", http_status=500)
        _add_suspend(script, "a1")
        _add_ok_task(script, "a2", "tk2", terminal="Done")
        _add_identity(script, "d1")
        _add_ok_task(script, "a3", "tk3", terminal="Done")
        _add_ok_task(script, "a4", "tk4", terminal="Done")

        server = ScriptedServer(script)
        self.addCleanup(server.close)
        ledger_path = self.tmp_path / "ledger.jsonl"

        run_arm(manifest, server.base_url, "C", "d1", ledger_path, poll_interval_s=0.0, task_deadline_s=5.0)

        rows = _read_ledger(ledger_path)
        phase1 = {r["task"]: r for r in rows if r.get("event") != "identity" and r["phase"] == 1}
        self.assertEqual(phase1["boom_task"]["status"], "driver-infra")
        self.assertEqual(phase1["boom_task"]["agent_id"], "a1")
        self.assertIsNone(phase1["boom_task"]["task_id"])  # submit never returned one
        self.assertEqual(phase1["next_task"]["status"], "Done")

        # The agent that failed submission was still suspended (invariant 2).
        suspend_paths = {c[1] for c in server.calls if c[0] == "POST" and c[1].endswith("/suspend")}
        self.assertIn("/agents/a1/suspend", suspend_paths)

    def test_poll_deadline_records_driver_infra_and_continues(self) -> None:
        names = ["slow_task", "ok_task"]
        manifest = _build_manifest(self.tmp_path, names)
        script = Script()
        _add_identity(script, "d1")
        _add_create(script, "a1")
        _add_submit(script, "a1", "tk1")
        _add_poll(script, "a1", "tk1", "Running", sticky=True)  # never terminates on its own
        _add_suspend(script, "a1")
        _add_ok_task(script, "a2", "tk2", terminal="Done")
        _add_identity(script, "d1")
        _add_ok_task(script, "a3", "tk3", terminal="Done")
        _add_ok_task(script, "a4", "tk4", terminal="Done")

        server = ScriptedServer(script)
        self.addCleanup(server.close)
        ledger_path = self.tmp_path / "ledger.jsonl"

        run_arm(
            manifest, server.base_url, "C", "d1", ledger_path, poll_interval_s=0.001, task_deadline_s=0.03
        )

        rows = _read_ledger(ledger_path)
        phase1 = {r["task"]: r for r in rows if r.get("event") != "identity" and r["phase"] == 1}
        self.assertEqual(phase1["slow_task"]["status"], "driver-infra")
        self.assertEqual(phase1["ok_task"]["status"], "Done")
        # The stuck agent was still suspended once the deadline gave up on it.
        suspend_paths = {c[1] for c in server.calls if c[0] == "POST" and c[1].endswith("/suspend")}
        self.assertIn("/agents/a1/suspend", suspend_paths)

    def test_identity_rows_written_each_phase_with_observed_digest(self) -> None:
        names = ["t0"]
        manifest = _build_manifest(self.tmp_path, names)
        script = Script()
        _add_identity(script, "d-real")
        _add_ok_task(script, "a1", "tk1")
        _add_identity(script, "d-real")
        _add_ok_task(script, "a2", "tk2")

        server = ScriptedServer(script)
        self.addCleanup(server.close)
        ledger_path = self.tmp_path / "ledger.jsonl"

        run_arm(
            manifest, server.base_url, "M", "d-real", ledger_path, poll_interval_s=0.0, task_deadline_s=5.0
        )

        rows = _read_ledger(ledger_path)
        identity_rows = [r for r in rows if r.get("event") == "identity"]
        self.assertEqual(len(identity_rows), 2)
        self.assertEqual([r["phase"] for r in identity_rows], [1, 2])
        self.assertTrue(all(r["digest"] == "d-real" for r in identity_rows))
        self.assertTrue(all(r["arm"] == "M" for r in identity_rows))
        self.assertTrue(all("ts" in r for r in identity_rows))

    def test_reset_restores_pristine_bytes_and_purges_pycache_between_phases(self) -> None:
        names = ["t0"]
        manifest = _build_manifest(self.tmp_path, names)
        workspace = Path(manifest["tasks"][0]["grant"]["write_roots"][0])

        # Simulate phase 1 having mutated the workspace (a landed patch, plus
        # leftover unittest bytecode) -- the exact shape a real daemon task
        # run leaves behind (memory-organ acceptance §2's own reset note).
        (workspace / "x.txt").write_text("MUTATED\n", encoding="utf-8")
        (workspace / "extra_generated.txt").write_text("junk\n", encoding="utf-8")
        pycache = workspace / "__pycache__"
        pycache.mkdir()
        (pycache / "x.cpython-312.pyc").write_bytes(b"stale bytecode")

        script = Script()
        _add_identity(script, "d1")
        _add_ok_task(script, "a1", "tk1")
        _add_identity(script, "d1")
        _add_ok_task(script, "a2", "tk2")

        server = ScriptedServer(script)
        self.addCleanup(server.close)
        ledger_path = self.tmp_path / "ledger.jsonl"

        run_arm(manifest, server.base_url, "C", "d1", ledger_path, poll_interval_s=0.0, task_deadline_s=5.0)

        self.assertEqual((workspace / "x.txt").read_text(encoding="utf-8"), "pristine-bytes\n")
        self.assertFalse((workspace / "extra_generated.txt").exists())
        self.assertFalse((workspace / "__pycache__").exists())


if __name__ == "__main__":
    unittest.main()
