import unittest

import roastlog


class TestRoastlog(unittest.TestCase):
    def test_highest_yield_grams(self):
        self.assertEqual(roastlog.highest_yield_grams([3, 1, 4, 1, 5]), 5)


if __name__ == "__main__":
    unittest.main()
