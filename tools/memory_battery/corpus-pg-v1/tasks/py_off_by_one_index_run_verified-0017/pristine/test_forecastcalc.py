import unittest

import forecastcalc


class TestForecastcalc(unittest.TestCase):
    def test_first_and_last_forecast_hour(self):
        self.assertEqual(forecastcalc.first_and_last_forecast_hour([7, 8, 9]), (7, 9))


if __name__ == "__main__":
    unittest.main()
