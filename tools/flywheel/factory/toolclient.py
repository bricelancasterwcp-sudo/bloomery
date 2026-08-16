"""A thin wrapper around ONE long-lived `flywheel-tool` subprocess.

Task 1's report flagged this explicitly: "the bin re-spawns nothing
between requests — it's a genuine one-process, line-at-a-time
stdin/stdout loop, so the factory should keep one long-lived subprocess
open and pipe thousands of lines through it rather than spawning
per-task." `generate.py` opens exactly one `ToolClient` per run and sends
every task's `trajectory` request through it.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path
from typing import Any


class ToolClient:
    """One JSON request per line on stdin, one JSON response per line on
    stdout — the wire protocol `flywheel-tool` (Task 1) speaks. `tool_path`
    ending in `.py` is invoked via the current interpreter (used by the
    test suite's stub tools); anything else is executed directly (the
    real compiled `flywheel-tool` binary)."""

    def __init__(self, tool_path: Path) -> None:
        self._tool_path = Path(tool_path)
        command = (
            [sys.executable, str(self._tool_path)]
            if self._tool_path.suffix == ".py"
            else [str(self._tool_path)]
        )
        self._proc = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )

    def trajectory(self, request: dict[str, Any]) -> dict[str, Any]:
        assert self._proc.stdin is not None
        assert self._proc.stdout is not None
        self._proc.stdin.write(json.dumps(request) + "\n")
        self._proc.stdin.flush()
        line = self._proc.stdout.readline()
        if not line:
            stderr_output = self._proc.stderr.read() if self._proc.stderr else ""
            raise RuntimeError(
                f"flywheel-tool subprocess ({self._tool_path}) closed stdout unexpectedly "
                f"(exit code {self._proc.poll()}); stderr: {stderr_output}"
            )
        return json.loads(line)

    def close(self) -> None:
        if self._proc.stdin is not None:
            self._proc.stdin.close()
        try:
            self._proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self._proc.kill()
            self._proc.wait()

    def __enter__(self) -> "ToolClient":
        return self

    def __exit__(self, exc_type: object, exc: object, tb: object) -> None:
        self.close()
