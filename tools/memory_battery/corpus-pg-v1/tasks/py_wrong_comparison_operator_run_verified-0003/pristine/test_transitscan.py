import unittest

import transitscan


class TestTransitscan(unittest.TestCase):
    def test_highest_declination(self):
        self.assertEqual(transitscan.highest_declination([3, 1, 4, 1, 5]), 5)


if __name__ == "__main__":
    unittest.main()
