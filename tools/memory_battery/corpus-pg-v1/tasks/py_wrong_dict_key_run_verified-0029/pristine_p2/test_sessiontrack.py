import unittest

import sessiontrack


class TestSessiontrack(unittest.TestCase):
    def test_treadmill_speed_value(self):
        self.assertEqual(sessiontrack.treadmill_speed_value({'primary': 1, 'secondary': 2, 'fallback': 3, 'override': 4, 'baseline': 5, 'candidate': 6, 'nominal': 7, 'adjusted': 8}), 3)


if __name__ == "__main__":
    unittest.main()
