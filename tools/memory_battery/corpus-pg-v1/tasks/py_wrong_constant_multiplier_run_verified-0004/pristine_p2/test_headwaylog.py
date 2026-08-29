import unittest

import headwaylog


class TestHeadwaylog(unittest.TestCase):
    def test_scaled_occupancy_ratio(self):
        self.assertEqual(headwaylog.scaled_occupancy_ratio(8), 12.0)


if __name__ == "__main__":
    unittest.main()
