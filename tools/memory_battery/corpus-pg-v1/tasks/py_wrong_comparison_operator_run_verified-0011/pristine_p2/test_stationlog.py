import unittest

import stationlog


class TestStationlog(unittest.TestCase):
    def test_highest_uv_index(self):
        self.assertEqual(stationlog.highest_uv_index([3, 1, 4, 1, 5]), 1)


if __name__ == "__main__":
    unittest.main()
