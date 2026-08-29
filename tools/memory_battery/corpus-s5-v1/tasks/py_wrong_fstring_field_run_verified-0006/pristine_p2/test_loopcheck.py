import unittest

import loopcheck


class TestLoopcheck(unittest.TestCase):
    def test_describe_channel_count(self):
        self.assertEqual(loopcheck.describe_channel_count(3, 5), 'channel_count=3, track_duration=5 (rev 2)')


if __name__ == "__main__":
    unittest.main()
