import unittest

import transitscan


class TestTransitscan(unittest.TestCase):
    def test_combined_luminosity_right_ascension(self):
        self.assertEqual(transitscan.combined_luminosity_right_ascension(3, 5), 12)


if __name__ == "__main__":
    unittest.main()
