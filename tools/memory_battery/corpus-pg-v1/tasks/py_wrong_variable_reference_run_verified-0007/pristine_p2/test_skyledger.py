import unittest

import skyledger


class TestSkyledger(unittest.TestCase):
    def test_combined_apparent_magnitude_transit_depth(self):
        self.assertEqual(skyledger.combined_apparent_magnitude_transit_depth(3, 5), 12)


if __name__ == "__main__":
    unittest.main()
