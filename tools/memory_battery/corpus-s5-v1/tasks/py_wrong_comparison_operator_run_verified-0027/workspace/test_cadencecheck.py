import unittest

import cadencecheck


class TestCadencecheck(unittest.TestCase):
    def test_lowest_heartrate_zone(self):
        self.assertEqual(cadencecheck.lowest_heartrate_zone([3, 1, 4, 1, 5]), 1)


if __name__ == "__main__":
    unittest.main()
