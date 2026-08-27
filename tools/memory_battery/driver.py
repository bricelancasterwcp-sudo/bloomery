"""The memory-battery driver (design spec §5; task-3 brief): drives one
arm's two phases against a live bloomery daemon over its native HTTP
surface -- create a fresh agent per task, submit the task, poll it to a
terminal status, suspend the agent -- in the frozen manifest's own order,
never retried, never reordered.

**Terminal-state table** (pinned). `_poll_task` returns as soon as one
poll response carries any status other than ``"Running"`` (the daemon's
``TaskStatus`` enum -- ``crates/bloomery-daemon/src/task/task_loop.rs`` --
serializes as ``Done``, ``BudgetExhausted``, ``StepsExhausted``,
``WindowExhausted``, or ``Error``, all handled identically here: whatever
the daemon says is the recorded status, verbatim). A poll deadline
(``task_deadline_s``, default 600s) or an HTTP-level failure at ANY of
create-agent / submit-task / poll is instead recorded as ``status:
"driver-infra"`` and the driver moves on to the next manifest task (design
spec §4's H3: infra is counted separately from task conduct, and a
dropped/infra task is never scored as a zero -- the none-vs-zero rule).

Only the internal ``_DriverInfra`` signal is caught this way. Any OTHER
exception (a malformed manifest entry, a genuine bug in this module)
propagates out of `run_arm` uncaught: a driver bug crashing the whole arm
run is correct behavior, not something to paper over as ordinary infra
noise -- the design spec's own >5%-infra-rate rule already treats "start
over from zero" as the answer to anything this serious, so there is no
value in a best-effort ledger row for a state nobody can vouch for.

**Identity assertion (plan ruling R-PF-B1).** Before EVERY phase's first
task, ``GET /status`` is read and its ``models[0]["digest"]`` is written
to the ledger as an ``identity`` row -- load-bearing, not decoration:
Task 4's recompute reads the served model's digest from these rows (no
journal `Event` carries a GGUF digest; it lives only in `/status`). A
digest that does not match the prereg's `expected_digest` raises
`IdentityMismatchError` and aborts the arm BEFORE that phase's first task
request -- no task POST, no wasted GPU-minutes on a boot serving the wrong
weights.

**Ledger.** One append-only JSONL file per arm (`Ledger`, flushed after
every row). Two row shapes, distinguished by the presence of `"event"`:

- task-half rows (one per task, per phase -- "half" because a task
  appears once in phase 1 and again in phase 2, and each appearance gets
  its own row): ``{"arm", "phase", "task", "agent_id", "task_id",
  "status", "wall_s", "suspend_ok", "ts"}``. ``suspend_ok`` is `True`/
  `False` when a suspend was attempted (HTTP 204 with no connection-level
  exception, vs. anything else -- a 404/409/500 suspend is fail-open by
  design, but is no longer silently indistinguishable from a clean one),
  or `None` when no agent was ever created for this task-half to begin
  with (task-3 review finding).
- identity rows (R-PF-B1, one per phase -- two per `run_arm` call):
  ``{"arm", "phase", "event": "identity", "digest", "ts"}``.

**The ledger is observational, never quotable as a cost or step number.**
`wall_s` is this driver's own wall-clock read on the create -> submit ->
poll cycle (it deliberately excludes the best-effort `suspend` call that
follows, since a cleanup call's latency is not part of the task's own
cost). Per design spec §5: "journal bytes are the only source any quoted
number may have" -- Task 4's recompute reads `tasks.jsonl` / `pager.jsonl`
directly for cost, steps, and status. This ledger exists only to (a) pair
phase-1/phase-2 task-halves by `(arm, phase, task)`, (b) flag driver-
detected infra breaks the journal alone cannot distinguish from a genuine
task failure, and (c) corroborate the served identity via the rows above.
No number in this file is ever the number a findings doc cites.

**Reset before EVERY phase.** Before phase 1's first request AND before
phase 2's first request, every task's workspace is restored to its frozen
`pristine/` byte-snapshot -- a full wipe-and-recopy (`_reset_workspace`),
so it is trivially byte-identical to `pristine/` and cannot retain a stray
`__pycache__` a previous `unittest` run may have left (`_purge_pycache`,
belt-and-suspenders on top of the wipe: the pyc rule -- stale bytecode
surviving a byte-only source reset is a documented hazard, see
`docs/superpowers/evidence/2026-08-26-memory-organ-acceptance.md` §2's own
reset recipe).

The PRE-PHASE-1 reset (branch-review finding I-1) is what makes an arm
independent of whatever ran before it: with resets only BETWEEN phases,
arm M's phase 1 would have started on the workspaces arm C's phase 2 left
patched -- silently turning M's "first exposure" into a second exposure on
already-fixed code and inverting H2's whole meaning. The loop is
idempotent by construction (wipe-and-recopy from `pristine/` is a
byte-identical no-op on an already-clean tree), so it costs nothing on the
first arm ever run and is the only thing that makes the second arm honest.

Python 3 stdlib only (`urllib.request`, `shutil`, `time`, `json`); no
network library beyond the standard library, no GPU access from this
module.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Sequence

# Design spec §2's pinned lens names the GGUF artifact:
# "model: qwen36-reap48-flywheel5-Q4_K_M.gguf ... window_cap: 16384 on
# every battery agent". `POST /agents`'s `model` field, however, is resolved
# by the daemon against its config's model TABLE -- an exact-key lookup
# against `BootConfig.models: BTreeMap<String, ModelSpec>`
# (`crates/bloomery-daemon/src/config.rs`), keyed by the boot TOML's stanza
# name (`[models."qwen36-reap48-flywheel5"]`), never the GGUF filename and
# with no alias/fallback -- a miss returns `PagerError::UnknownModel` -> 404
# (`crates/bloomery-daemon/src/api_native.rs`/`api_v1.rs`). MODEL is
# therefore the daemon API model NAME (the stanza key), not the artifact
# filename the spec's lens line names -- task-5 review finding C1, fixed
# here after the original judgment call posted the GGUF filename and would
# have 404'd every task on both boots. `run_arm`'s signature is pinned by
# the task-3 brief to its exact five positional parameters (manifest,
# base_url, arm_name, expected_digest, ledger_path) -- the model name and
# window cap are therefore instrument-wide module constants here (the same
# role corpus.py's INSTRUMENT / PYTHON_LENS play), not per-call arguments,
# since every task in every arm uses the identical one.
MODEL = "qwen36-reap48-flywheel5"
WINDOW_CAP = 16384

# Design spec §5 / task-3 brief: 5s poll cadence, 600s per-task deadline --
# the real defaults. Both are keyword-only parameters on `run_arm` (never
# positional -- the brief's five positional args stay exactly as pinned) so
# the fake-server tests can pass tiny values and run in milliseconds with
# real `time.sleep` / `time.monotonic()` calls, no mocking of time needed.
DEFAULT_POLL_INTERVAL_S = 5.0
DEFAULT_TASK_DEADLINE_S = 600.0

DRIVER_INFRA_STATUS = "driver-infra"


class IdentityMismatchError(RuntimeError):
    """Raised by `_assert_identity` when `/status`'s served
    `models[0]["digest"]` does not match the prereg's `expected_digest`
    (or `/status` could not be read at all) -- the arm aborts before any
    task request, per R-PF-B1."""


class _DriverInfra(RuntimeError):
    """Internal signal only: an HTTP call failed, or a poll deadline
    elapsed, for the CURRENT task. Caught by `_process_task` and turned
    into a `"driver-infra"` ledger row -- never propagates out of
    `run_arm`."""


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _http_json(
    method: str, url: str, body: dict[str, Any] | None = None, timeout: float = 30.0
) -> tuple[int, Any]:
    """One JSON-in/JSON-out HTTP call. An HTTP-level error status (4xx/5xx)
    is returned as a normal `(status, payload)` pair -- `urllib` raises
    `HTTPError` for those, caught and unwrapped here so every caller has
    one code path for "the daemon answered, just not happily". A
    connection-level failure (`URLError`/`OSError` -- refused, reset,
    timed out) is deliberately NOT caught here: it propagates to the
    caller, which decides what that means for its own retry-never
    contract (`_DriverInfra` for a task call, `IdentityMismatchError` for
    an identity assert)."""
    data = json.dumps(body).encode("utf-8") if body is not None else None
    request = urllib.request.Request(url, data=data, method=method)
    if data is not None:
        request.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            raw = response.read()
            return response.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as error:
        # HTTPError is itself the (unclosed-by-default) response object --
        # `with error:` releases its underlying socket instead of leaking it
        # (surfaced as a ResourceWarning under a poll loop that hits this
        # path repeatedly, e.g. this module's own scripted-500 test).
        with error:
            raw = error.read()
            code = error.code
        try:
            payload = json.loads(raw) if raw else None
        except json.JSONDecodeError:
            payload = None
        return code, payload


def _assert_identity(base_url: str, arm: str, phase: int, expected_digest: str, ledger: "Ledger") -> None:
    """The R-PF-B1 identity assert: reads `/status`, writes the identity
    ledger row with whatever digest was observed (even a mismatched one --
    the row is what corroborates the mismatch), then raises
    `IdentityMismatchError` if it does not equal `expected_digest`. A
    connection-level failure raises immediately WITHOUT a ledger row
    (there was no response to observe a digest from, a materially
    different failure mode than "responded with the wrong digest")."""
    try:
        status, payload = _http_json("GET", f"{base_url}/status")
    except (urllib.error.URLError, OSError) as error:
        raise IdentityMismatchError(
            f"{arm} phase {phase}: GET /status failed before this phase's first task: {error}"
        ) from error

    digest = None
    if status == 200 and isinstance(payload, dict):
        models = payload.get("models") or []
        if models and isinstance(models[0], dict):
            digest = models[0].get("digest")

    ledger.append({"arm": arm, "phase": phase, "event": "identity", "digest": digest, "ts": _now_iso()})

    if digest != expected_digest:
        raise IdentityMismatchError(
            f"{arm} phase {phase}: served digest {digest!r} != expected {expected_digest!r} "
            f"(/status returned HTTP {status})"
        )


def _create_agent(base_url: str) -> str:
    try:
        status, payload = _http_json("POST", f"{base_url}/agents", {"model": MODEL, "window_cap": WINDOW_CAP})
    except (urllib.error.URLError, OSError) as error:
        raise _DriverInfra(f"POST /agents failed: {error}") from error
    if status != 201 or not isinstance(payload, dict) or "id" not in payload:
        raise _DriverInfra(f"POST /agents returned unexpected response: HTTP {status} {payload!r}")
    return payload["id"]


def _submit_task(base_url: str, agent_id: str, goal: str, grant: dict[str, Any]) -> str:
    try:
        status, payload = _http_json(
            "POST", f"{base_url}/agents/{agent_id}/task", {"goal": goal, "grants": grant}
        )
    except (urllib.error.URLError, OSError) as error:
        raise _DriverInfra(f"POST /agents/{agent_id}/task failed: {error}") from error
    if status != 202 or not isinstance(payload, dict) or "task_id" not in payload:
        raise _DriverInfra(
            f"POST /agents/{agent_id}/task returned unexpected response: HTTP {status} {payload!r}"
        )
    return payload["task_id"]


def _poll_task(base_url: str, agent_id: str, task_id: str, poll_interval_s: float, task_deadline_s: float) -> str:
    """Polls until a status other than `"Running"` is observed, or raises
    `_DriverInfra` once `task_deadline_s` elapses. The deadline is checked
    BEFORE each request (never mid-sleep), and there is no extra sleep
    after a terminal status is already in hand."""
    deadline = time.monotonic() + task_deadline_s
    url = f"{base_url}/agents/{agent_id}/task/{task_id}"
    while True:
        if time.monotonic() > deadline:
            raise _DriverInfra(f"poll deadline ({task_deadline_s}s) exceeded for task {task_id}")
        try:
            status, payload = _http_json("GET", url)
        except (urllib.error.URLError, OSError) as error:
            raise _DriverInfra(f"GET {url} failed: {error}") from error
        if status != 200 or not isinstance(payload, dict) or "status" not in payload:
            raise _DriverInfra(f"GET {url} returned unexpected response: HTTP {status} {payload!r}")
        task_status = payload["status"]
        if task_status != "Running":
            return task_status
        time.sleep(poll_interval_s)


def _suspend(base_url: str, agent_id: str) -> bool:
    """Best-effort: the driver never retries, and a suspend failure does
    not change the task's already-recorded status (that status reflects
    the task's own poll outcome, unaffected by whether cleanup succeeded --
    see this module's docstring on the ledger being observational).
    Returns whether it actually landed (HTTP 204, no connection-level
    exception) -- fed into the ledger row's `suspend_ok` field so a
    silently-swallowed 404/409/500 here stays legible instead of being
    indistinguishable from a clean suspend (task-3 review finding)."""
    try:
        status, _payload = _http_json("POST", f"{base_url}/agents/{agent_id}/suspend")
    except (urllib.error.URLError, OSError):
        return False
    return status == 204


def _reset_workspace(workspace_dir: Path) -> None:
    """Restores `workspace_dir` to its sibling `pristine/` snapshot
    (corpus.py's own on-disk shape: `tasks/<name>/{workspace,pristine}/`)
    via a full wipe-and-recopy -- byte-identical to pristine by
    construction. `_purge_pycache` runs afterward as explicit,
    independently-named belt-and-suspenders coverage of the pyc rule (see
    this module's docstring)."""
    workspace_dir = Path(workspace_dir)
    pristine_dir = workspace_dir.parent / "pristine"
    if workspace_dir.exists():
        shutil.rmtree(workspace_dir)
    shutil.copytree(pristine_dir, workspace_dir)
    _purge_pycache(workspace_dir)


def _purge_pycache(directory: Path) -> None:
    """Removes every `__pycache__` directory under `directory` -- stale
    bytecode surviving a byte-only source reset can run mutated code after
    the source was restored, a documented hazard (this module's
    docstring)."""
    for cache_dir in directory.rglob("__pycache__"):
        shutil.rmtree(cache_dir, ignore_errors=True)


class Ledger:
    """Append-only JSONL ledger, one file per arm, flushed after every
    row -- an interrupted run's partial ledger is still fully readable up
    to the last row it managed to write. See this module's docstring:
    every number in here is observational, never quotable; Task 4's
    recompute reads `tasks.jsonl` / `pager.jsonl` directly for anything
    that is."""

    def __init__(self, path: Path) -> None:
        self._path = Path(path)
        self._path.parent.mkdir(parents=True, exist_ok=True)
        self._file = self._path.open("a", encoding="utf-8")

    def append(self, row: dict[str, Any]) -> None:
        self._file.write(json.dumps(row, sort_keys=True) + "\n")
        self._file.flush()

    def close(self) -> None:
        self._file.close()


def _process_task(
    base_url: str,
    arm: str,
    phase: int,
    task_entry: dict[str, Any],
    poll_interval_s: float,
    task_deadline_s: float,
    ledger: Ledger,
) -> None:
    """One task-half: create agent -> submit -> poll to terminal, suspend
    the agent if one was created (regardless of outcome -- suspend runs
    for a poll-deadline / HTTP-failure task-half too, not only a clean
    one), then append exactly one ledger row. Only `_DriverInfra` is
    caught (see this module's docstring); anything else propagates out of
    `run_arm` uncaught.

    `wall_s` is measured up to the terminal status only -- BEFORE the
    `suspend` call below, deliberately, so a slow or failing suspend never
    inflates a task's own recorded wall time (see this module's docstring:
    a cleanup call's latency is not part of the task's cost)."""
    start = time.monotonic()
    ts = _now_iso()
    agent_id: str | None = None
    task_id: str | None = None
    try:
        agent_id = _create_agent(base_url)
        task_id = _submit_task(base_url, agent_id, task_entry["goal"], task_entry["grant"])
        status = _poll_task(base_url, agent_id, task_id, poll_interval_s, task_deadline_s)
    except _DriverInfra:
        status = DRIVER_INFRA_STATUS
    wall_s = time.monotonic() - start

    # `suspend_ok` is `None` when no agent was ever created (nothing to
    # suspend -- distinct from `False`, which means suspend was attempted
    # and the daemon refused/errored it).
    suspend_ok: bool | None = None
    if agent_id is not None:
        suspend_ok = _suspend(base_url, agent_id)

    ledger.append(
        {
            "arm": arm,
            "phase": phase,
            "task": task_entry["name"],
            "agent_id": agent_id,
            "task_id": task_id,
            "status": status,
            "wall_s": wall_s,
            "suspend_ok": suspend_ok,
            "ts": ts,
        }
    )


def run_arm(
    manifest: dict[str, Any],
    base_url: str,
    arm_name: str,
    expected_digest: str,
    ledger_path: Path,
    *,
    poll_interval_s: float = DEFAULT_POLL_INTERVAL_S,
    task_deadline_s: float = DEFAULT_TASK_DEADLINE_S,
) -> None:
    """Runs one arm's full two-phase protocol (design spec §4/§5) against
    the daemon at `base_url`: a full workspace reset, phase 1 in manifest
    order, another full reset, phase 2 in the same order with fresh
    agents. See this module's docstring for the terminal-state table, the
    ledger's two row shapes, the identity-assert/abort rule (R-PF-B1), and
    why the PRE-PHASE-1 reset exists (branch-review finding I-1: without
    it, arm M's phase 1 inherits arm C's phase-2 patched workspaces).

    Raises `IdentityMismatchError` if either phase's served digest does
    not match `expected_digest` -- always BEFORE that phase's first task
    request. Never raises for an individual task's own failure or for a
    driver-infra event; those are recorded in the ledger and the run
    continues (the terminal-state table). Never retries a task, never
    reorders the manifest."""
    tasks = manifest["tasks"]
    ledger = Ledger(ledger_path)
    try:
        # Pre-phase-1 reset (finding I-1). Idempotent: a wipe-and-recopy
        # from `pristine/` is a byte-identical no-op on an already-clean
        # tree, so this is free on the first arm and load-bearing on the
        # second. It runs BEFORE the phase-1 identity assert so that no
        # request of any kind is issued against a stale workspace.
        for task_entry in tasks:
            _reset_workspace(Path(task_entry["grant"]["write_roots"][0]))

        _assert_identity(base_url, arm_name, 1, expected_digest, ledger)
        for task_entry in tasks:
            _process_task(base_url, arm_name, 1, task_entry, poll_interval_s, task_deadline_s, ledger)

        for task_entry in tasks:
            _reset_workspace(Path(task_entry["grant"]["write_roots"][0]))

        _assert_identity(base_url, arm_name, 2, expected_digest, ledger)
        for task_entry in tasks:
            _process_task(base_url, arm_name, 2, task_entry, poll_interval_s, task_deadline_s, ledger)
    finally:
        ledger.close()


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "memory-battery driver (design spec §5; task-3 brief): runs one arm's two "
            "phases against a live bloomery daemon over its native HTTP surface, in "
            "frozen manifest order, fresh agent per task, never retried."
        )
    )
    parser.add_argument("--manifest", type=Path, required=True, help="Path to the frozen corpus's manifest.json.")
    parser.add_argument("--base-url", required=True, help="Daemon base URL, e.g. http://127.0.0.1:8397.")
    parser.add_argument("--arm", required=True, help="Arm name recorded on every ledger row (e.g. C or M).")
    parser.add_argument(
        "--expected-digest",
        required=True,
        help="Pinned served-identity digest (design spec §2); a mismatch aborts the arm before any task request.",
    )
    parser.add_argument("--ledger", type=Path, required=True, help="Output path for this arm's JSONL ledger.")
    parser.add_argument(
        "--pid-file",
        type=Path,
        default=None,
        help=(
            "Written with this process's own real pid at startup, before anything else runs. "
            "run_battery.sh's detach wrapper relies on this rather than a captured `$!`: `setsid` "
            "double-forks when the invoking shell is already a process-group leader, so `$!` can "
            "name an already-exited intermediate process -- see "
            "docs/superpowers/evidence/2026-08-26-memory-organ-acceptance.md's own "
            '"real PID ... found via ps (never $!)" note.'
        ),
    )
    parser.add_argument("--poll-interval-s", type=float, default=DEFAULT_POLL_INTERVAL_S)
    parser.add_argument("--task-deadline-s", type=float, default=DEFAULT_TASK_DEADLINE_S)
    args = parser.parse_args(argv)

    if args.pid_file is not None:
        args.pid_file.parent.mkdir(parents=True, exist_ok=True)
        args.pid_file.write_text(str(os.getpid()), encoding="utf-8")

    # Last-resort net matching corpus_check.py's main() house pattern: this
    # CLI always prints one legible, named failure line and exits nonzero,
    # never a raw traceback -- run_battery.sh's DONE marker still gets a
    # real exit code either way, but a clean message is worth the four lines.
    try:
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
        run_arm(
            manifest,
            args.base_url,
            args.arm,
            args.expected_digest,
            args.ledger,
            poll_interval_s=args.poll_interval_s,
            task_deadline_s=args.task_deadline_s,
        )
    except Exception as exc:  # noqa: BLE001 -- deliberately broad, see comment above
        print(f"memory_battery.driver: FATAL: {exc!r}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
