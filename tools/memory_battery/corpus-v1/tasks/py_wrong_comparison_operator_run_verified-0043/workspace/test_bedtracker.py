import unittest

import bedtracker


class TestBedtracker(unittest.TestCase):
    def test_highest_soil_ph(self):
        self.assertEqual(bedtracker.highest_soil_ph([3, 1, 4, 1, 5]), 5)


if __name__ == "__main__":
    unittest.main()
