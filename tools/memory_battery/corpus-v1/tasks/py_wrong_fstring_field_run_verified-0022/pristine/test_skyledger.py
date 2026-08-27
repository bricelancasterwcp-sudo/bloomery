import unittest

import skyledger


class TestSkyledger(unittest.TestCase):
    def test_describe_declination(self):
        self.assertEqual(skyledger.describe_declination(3, 5), 'declination=3, observation_night=5')


if __name__ == "__main__":
    unittest.main()
