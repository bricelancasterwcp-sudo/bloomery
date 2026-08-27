import unittest

import transitscan


class TestTransitscan(unittest.TestCase):
    def test_apparent_magnitude_value(self):
        self.assertEqual(transitscan.apparent_magnitude_value({'primary': 1, 'secondary': 2, 'fallback': 3, 'override': 4, 'baseline': 5, 'candidate': 6, 'nominal': 7, 'adjusted': 8}), 5)


if __name__ == "__main__":
    unittest.main()
