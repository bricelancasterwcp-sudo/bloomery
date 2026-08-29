import unittest

import loopcheck


class TestLoopcheck(unittest.TestCase):
    def test_highest_tempo_bpm(self):
        self.assertEqual(loopcheck.highest_tempo_bpm([3, 1, 4, 1, 5]), 5)


if __name__ == "__main__":
    unittest.main()
