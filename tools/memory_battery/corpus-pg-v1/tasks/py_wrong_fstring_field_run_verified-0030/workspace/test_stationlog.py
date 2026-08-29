import unittest

import stationlog


class TestStationlog(unittest.TestCase):
    def test_describe_visibility(self):
        self.assertEqual(stationlog.describe_visibility(3, 5), 'visibility=3, cloud_cover=5')


if __name__ == "__main__":
    unittest.main()
