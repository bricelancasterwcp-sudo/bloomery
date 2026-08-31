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

import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from openai_tools.bloomery import BloomeryClient
from openai_tools.errors import BloomeryError


class _Stub(BaseHTTPRequestHandler):
    routes = {}

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        self.rfile.read(length)
        status, payload = self.routes.get(self.path, (404, {"error": "no route"}))
        raw = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def log_message(self, *args):
        pass


def _serve(routes):
    _Stub.routes = routes
    srv = ThreadingHTTPServer(("127.0.0.1", 0), _Stub)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv


class BloomeryClientTest(unittest.TestCase):
    def test_create_agent_returns_the_id_the_daemon_assigned(self):
        srv = _serve({"/agents": (200, {"id": "a7", "window_tokens": 103124,
                                        "bound_by": "vram"})})
        try:
            client = BloomeryClient(f"http://127.0.0.1:{srv.server_port}")
            self.assertEqual(client.create_agent("m"), "a7")
        finally:
            srv.shutdown()

    def test_infer_returns_the_reply_body(self):
        srv = _serve({"/agents/a7/infer": (200, {
            "text": "hello", "prompt_tokens": 8, "completion_tokens": 2,
            "duration_ms": 12})})
        try:
            client = BloomeryClient(f"http://127.0.0.1:{srv.server_port}")
            self.assertEqual(client.infer("a7", "p", 16)["text"], "hello")
        finally:
            srv.shutdown()

    def test_a_refusal_is_raised_as_BloomeryError_carrying_the_body(self):
        srv = _serve({"/agents/a7/infer": (413, {
            "error": "prompt_too_large", "needed_tokens": 9, "window_tokens": 4})})
        try:
            client = BloomeryClient(f"http://127.0.0.1:{srv.server_port}")
            with self.assertRaises(BloomeryError) as caught:
                client.infer("a7", "p", 16)
            self.assertEqual(caught.exception.status, 413)
            self.assertEqual(caught.exception.body["needed_tokens"], 9)
        finally:
            srv.shutdown()


if __name__ == "__main__":
    unittest.main()
