import unittest

import forecastcalc


class TestForecastcalc(unittest.TestCase):
    def test_scaled_humidity(self):
        self.assertEqual(forecastcalc.scaled_humidity(8), 10.0)


if __name__ == "__main__":
    unittest.main()
