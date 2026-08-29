import unittest

import switchbacks


class TestSwitchbacks(unittest.TestCase):
    def test_lowest_ranger_station(self):
        self.assertEqual(switchbacks.lowest_ranger_station([3, 1, 4, 1, 5]), 12)


if __name__ == "__main__":
    unittest.main()
