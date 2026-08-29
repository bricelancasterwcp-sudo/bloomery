import unittest

import weathercheck


class TestWeathercheck(unittest.TestCase):
    def test_combined_visibility_dew_point(self):
        self.assertEqual(weathercheck.combined_visibility_dew_point(3, 5), 16)


if __name__ == "__main__":
    unittest.main()
