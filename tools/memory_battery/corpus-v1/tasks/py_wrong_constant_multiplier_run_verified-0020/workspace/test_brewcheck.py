import unittest

import brewcheck


class TestBrewcheck(unittest.TestCase):
    def test_scaled_extraction_time(self):
        self.assertEqual(brewcheck.scaled_extraction_time(8), 32.0)


if __name__ == "__main__":
    unittest.main()
