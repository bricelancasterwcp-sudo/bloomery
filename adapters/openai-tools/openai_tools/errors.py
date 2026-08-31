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

"""bloomery refusals, translated without losing the arithmetic.

bloomery's refusals carry numbers — bytes needed and free, tokens needed and
available. Those numbers are the point: a client that receives them can act,
where a bare 500 leaves it guessing. Nothing here rounds them off.
"""


class BloomeryError(Exception):
    def __init__(self, status: int, body):
        super().__init__(f"bloomery {status}: {body}")
        self.status = status
        # Normalize body to dict so to_openai_error can safely call .get()
        if isinstance(body, dict):
            self.body = body
        else:
            self.body = {"error": str(body)} if body else {}


def _envelope(kind: str, code: str, message: str) -> dict:
    return {"error": {"type": kind, "code": code, "message": message}}


def to_openai_error(err: BloomeryError) -> tuple[int, dict]:
    body, kind = err.body, err.body.get("error", "")
    if err.status == 413 or kind == "prompt_too_large":
        return 413, _envelope(
            "invalid_request_error", "context_length_exceeded",
            f"prompt needs {body.get('needed_tokens')} tokens; the agent's window "
            f"is {body.get('window_tokens')}. bloomery refuses rather than truncating.")
    if err.status == 409 or kind == "refused":
        return 409, _envelope(
            "server_error", "residency_refused",
            f"the model could not be made resident: needed {body.get('needed')} B, "
            f"free {body.get('free')} B, reclaimable {body.get('reclaimable')} B.")
    if err.status == 402 or kind == "budget":
        return 402, _envelope("insufficient_quota", "insufficient_quota",
                              f"token budget exhausted: {body}")
    if err.status == 404:
        return 404, _envelope("invalid_request_error", "model_not_found", f"{body}")
    if kind == "unprofiled":
        # Fix wave, Important 2: the single likeliest FIRST error on a live
        # run (crates/bloomery-daemon/src/api_native.rs maps
        # PagerError::Unprofiled to a native 422 carrying {error, model}).
        # A bare 502 upstream_error would misreport this as a substrate
        # protocol breach; it is a normal, nameable admission refusal.
        return 422, _envelope(
            "invalid_request_error", "model_unprofiled",
            f"model {body.get('model')!r} has no capability profile yet -- "
            "bloomery refuses to serve an unprofiled model rather than guessing.")
    if kind == "drift_blocked":
        # Spec §7: DriftBlocked -> 409, naming the blocked model (the
        # native API's own status for this is 422; this adapter's error
        # envelope is a deliberate translation, not a passthrough -- see
        # this module's docstring on why the numbers are kept, not the
        # daemon's raw status code).
        return 409, _envelope(
            "server_error", "drift_blocked",
            f"model {body.get('model')!r} is blocked pending drift review "
            f"(reference {body.get('reference')!r}).")
    if err.status == 503 or kind == "connection_failed":
        return 503, _envelope(
            "server_error", "unavailable",
            f"bloomery daemon is unreachable: {body.get('detail', 'connection refused')}")
    return 502, _envelope("server_error", "upstream_error",
                          f"bloomery returned {err.status}: {body}")
