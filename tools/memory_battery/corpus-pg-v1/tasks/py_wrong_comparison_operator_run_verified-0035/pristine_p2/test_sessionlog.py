import unittest

import sessionlog


class TestSessionlog(unittest.TestCase):
    def test_highest_beat_offset(self):
        self.assertEqual(sessionlog.highest_beat_offset([3, 1, 4, 1, 5]), 1)


if __name__ == "__main__":
    unittest.main()
