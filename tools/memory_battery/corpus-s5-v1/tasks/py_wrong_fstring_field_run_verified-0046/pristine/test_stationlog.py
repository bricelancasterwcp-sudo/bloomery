import unittest

import stationlog


class TestStationlog(unittest.TestCase):
    def test_describe_uv_index(self):
        self.assertEqual(stationlog.describe_uv_index(3, 5), 'uv_index=3, forecast_hour=5')


if __name__ == "__main__":
    unittest.main()
