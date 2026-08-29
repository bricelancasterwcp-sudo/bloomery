import unittest

import trailplan


class TestTrailplan(unittest.TestCase):
    def test_ranger_station_checkpoints(self):
        self.assertEqual(trailplan.ranger_station_checkpoints(3), ['cycle 1', 'cycle 2', 'cycle 3'])


if __name__ == "__main__":
    unittest.main()
