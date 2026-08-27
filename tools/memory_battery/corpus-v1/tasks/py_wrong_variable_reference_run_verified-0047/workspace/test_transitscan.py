import unittest

import transitscan


class TestTransitscan(unittest.TestCase):
    def test_combined_right_ascension_angular_size(self):
        self.assertEqual(transitscan.combined_right_ascension_angular_size(3, 5), 16)


if __name__ == "__main__":
    unittest.main()
