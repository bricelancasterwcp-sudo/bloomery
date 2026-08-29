import unittest

import dwelltrack


class TestDwelltrack(unittest.TestCase):
    def test_transfer_window_value(self):
        self.assertEqual(dwelltrack.transfer_window_value({'primary': 1, 'secondary': 2, 'fallback': 3, 'override': 4, 'baseline': 5, 'candidate': 6, 'nominal': 7, 'adjusted': 8}), 15)


if __name__ == "__main__":
    unittest.main()
