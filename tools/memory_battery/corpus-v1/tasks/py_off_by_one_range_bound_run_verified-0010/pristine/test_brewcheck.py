import unittest

import brewcheck


class TestBrewcheck(unittest.TestCase):
    def test_yield_grams_checkpoints(self):
        self.assertEqual(brewcheck.yield_grams_checkpoints(3), ['cycle 1', 'cycle 2', 'cycle 3'])


if __name__ == "__main__":
    unittest.main()
