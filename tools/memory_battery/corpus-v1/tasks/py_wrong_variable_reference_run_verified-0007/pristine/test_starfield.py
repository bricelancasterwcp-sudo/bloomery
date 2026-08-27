import unittest

import starfield


class TestStarfield(unittest.TestCase):
    def test_combined_apparent_magnitude_transit_depth(self):
        self.assertEqual(starfield.combined_apparent_magnitude_transit_depth(3, 5), 16)


if __name__ == "__main__":
    unittest.main()
