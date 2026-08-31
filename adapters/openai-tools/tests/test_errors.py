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

import unittest

from openai_tools.errors import BloomeryError, to_openai_error


class ErrorMappingTest(unittest.TestCase):
    def test_oversize_becomes_context_length_exceeded_and_keeps_the_arithmetic(self):
        status, body = to_openai_error(BloomeryError(413, {
            "error": "prompt_too_large", "needed_tokens": 120000,
            "window_tokens": 103124}))
        self.assertEqual(status, 413)
        self.assertEqual(body["error"]["code"], "context_length_exceeded")
        self.assertIn("120000", body["error"]["message"])
        self.assertIn("103124", body["error"]["message"])

    def test_residency_refusal_keeps_the_bytes_it_was_refused_over(self):
        status, body = to_openai_error(BloomeryError(409, {
            "error": "refused", "needed": 2611945472, "free": 1925343488,
            "reclaimable": 0}))
        self.assertEqual(status, 409)
        self.assertIn("2611945472", body["error"]["message"])

    def test_budget_exhaustion_is_insufficient_quota(self):
        status, body = to_openai_error(BloomeryError(402, {"error": "budget"}))
        self.assertEqual(status, 402)
        self.assertEqual(body["error"]["code"], "insufficient_quota")

    def test_an_unmapped_status_is_surfaced_not_swallowed(self):
        status, body = to_openai_error(BloomeryError(500, {"error": "weird"}))
        self.assertEqual(status, 502)
        self.assertIn("weird", body["error"]["message"])

    def test_json_list_body_is_normalised_not_raising(self):
        status, body = to_openai_error(BloomeryError(500, [1, 2, 3]))
        self.assertEqual(status, 502)
        self.assertIn("1", body["error"]["message"])

    def test_non_empty_string_body_is_normalised_not_raising(self):
        status, body = to_openai_error(BloomeryError(500, "something failed"))
        self.assertEqual(status, 502)
        self.assertIn("something failed", body["error"]["message"])

    def test_bare_number_body_is_normalised_not_raising(self):
        status, body = to_openai_error(BloomeryError(500, 42))
        self.assertEqual(status, 502)
        self.assertIn("42", body["error"]["message"])

    def test_empty_dict_body_does_not_raise(self):
        status, body = to_openai_error(BloomeryError(500, {}))
        self.assertEqual(status, 502)


if __name__ == "__main__":
    unittest.main()
