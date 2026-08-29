import unittest

import reeflog


class TestReeflog(unittest.TestCase):
    def test_describe_water_temp(self):
        self.assertEqual(reeflog.describe_water_temp(3, 5), 'water_temp=3, algae_growth=5')


if __name__ == "__main__":
    unittest.main()
