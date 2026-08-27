import unittest

import summitlog


class TestSummitlog(unittest.TestCase):
    def test_water_capacity_value(self):
        self.assertEqual(summitlog.water_capacity_value({'primary': 1, 'secondary': 2, 'fallback': 3, 'override': 4, 'baseline': 5, 'candidate': 6, 'nominal': 7, 'adjusted': 8}), 4)


if __name__ == "__main__":
    unittest.main()
