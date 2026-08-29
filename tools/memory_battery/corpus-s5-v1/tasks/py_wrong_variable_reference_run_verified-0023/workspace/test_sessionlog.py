import unittest

import sessionlog


class TestSessionlog(unittest.TestCase):
    def test_combined_track_duration_session_id(self):
        self.assertEqual(sessionlog.combined_track_duration_session_id(3, 5), 16)


if __name__ == "__main__":
    unittest.main()
