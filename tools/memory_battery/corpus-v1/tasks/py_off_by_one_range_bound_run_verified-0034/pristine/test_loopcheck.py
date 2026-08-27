import unittest

import loopcheck


class TestLoopcheck(unittest.TestCase):
    def test_crossfade_ms_checkpoints(self):
        self.assertEqual(loopcheck.crossfade_ms_checkpoints(3), ['cycle 1', 'cycle 2', 'cycle 3'])


if __name__ == "__main__":
    unittest.main()
