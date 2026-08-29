import unittest

import climatetrend


class TestClimatetrend(unittest.TestCase):
    def test_rainfall_checkpoints(self):
        self.assertEqual(climatetrend.rainfall_checkpoints(3), ['cycle 1', 'cycle 2'])


if __name__ == "__main__":
    unittest.main()
