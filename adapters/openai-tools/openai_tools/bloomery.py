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

"""A small client for bloomery's native agent API.

Native rather than `/v1`: the adapter must own the prompt bytes exactly, and
`/v1`'s `fallback_prompt` would rewrite them into `"role: content"` lines.
"""
import json
import urllib.error
import urllib.request

from .errors import BloomeryError


class BloomeryClient:
    def __init__(self, base_url: str, timeout: float = 600.0):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    def _post(self, path: str, payload: dict) -> dict:
        raw = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(
            f"{self.base_url}{path}", data=raw,
            headers={"Content-Type": "application/json"}, method="POST")
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                text = resp.read().decode("utf-8")
                return json.loads(text) if text else {}
        except urllib.error.HTTPError as exc:
            text = exc.read().decode("utf-8", errors="replace")
            try:
                body = json.loads(text)
            except json.JSONDecodeError:
                body = {"error": text}
            raise BloomeryError(exc.code, body) from exc

    def create_agent(self, model: str, window_cap: int | None = None) -> str:
        payload: dict = {"model": model}
        if window_cap is not None:
            payload["window_cap"] = window_cap
        return self._post("/agents", payload)["id"]

    def infer(self, agent_id: str, prompt: str, max_tokens: int) -> dict:
        return self._post(f"/agents/{agent_id}/infer",
                          {"prompt": prompt, "max_tokens": max_tokens})

    def suspend(self, agent_id: str) -> None:
        self._post(f"/agents/{agent_id}/suspend", {})
