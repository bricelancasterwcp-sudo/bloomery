import unittest

import climatetrend


class TestClimatetrend(unittest.TestCase):
    def test_dew_point_checkpoints(self):
        self.assertEqual(climatetrend.dew_point_checkpoints(3), ['cycle 1', 'cycle 2', 'cycle 3'])


if __name__ == "__main__":
    unittest.main()
