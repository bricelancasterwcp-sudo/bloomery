import unittest

import reeflog


class TestReeflog(unittest.TestCase):
    def test_scaled_water_temp(self):
        self.assertEqual(reeflog.scaled_water_temp(8), 16.0)


if __name__ == "__main__":
    unittest.main()
