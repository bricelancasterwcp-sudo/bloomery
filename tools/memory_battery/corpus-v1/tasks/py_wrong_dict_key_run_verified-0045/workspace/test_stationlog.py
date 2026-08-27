import unittest

import stationlog


class TestStationlog(unittest.TestCase):
    def test_temperature_value(self):
        self.assertEqual(stationlog.temperature_value({'primary': 1, 'secondary': 2, 'fallback': 3, 'override': 4, 'baseline': 5, 'candidate': 6, 'nominal': 7, 'adjusted': 8}), 4)


if __name__ == "__main__":
    unittest.main()
