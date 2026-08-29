import unittest

import transitscan


class TestTransitscan(unittest.TestCase):
    def test_lowest_orbital_period(self):
        self.assertEqual(transitscan.lowest_orbital_period([3, 1, 4, 1, 5]), 1)


if __name__ == "__main__":
    unittest.main()
