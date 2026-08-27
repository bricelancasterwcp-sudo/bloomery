import unittest

import growcheck


class TestGrowcheck(unittest.TestCase):
    def test_combined_sunlight_hours_soil_ph(self):
        self.assertEqual(growcheck.combined_sunlight_hours_soil_ph(3, 5), 16)


if __name__ == "__main__":
    unittest.main()
