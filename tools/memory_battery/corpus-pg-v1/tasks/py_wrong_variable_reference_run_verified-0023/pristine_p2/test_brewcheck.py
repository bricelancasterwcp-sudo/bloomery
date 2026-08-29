import unittest

import brewcheck


class TestBrewcheck(unittest.TestCase):
    def test_combined_yield_grams_bean_weight(self):
        self.assertEqual(brewcheck.combined_yield_grams_bean_weight(3, 5), 12)


if __name__ == "__main__":
    unittest.main()
