import unittest

import switchbacks


class TestSwitchbacks(unittest.TestCase):
    def test_switchback_count_value(self):
        self.assertEqual(switchbacks.switchback_count_value({'primary': 1, 'secondary': 2, 'fallback': 3, 'override': 4, 'baseline': 5, 'candidate': 6, 'nominal': 7, 'adjusted': 8}), 6)


if __name__ == "__main__":
    unittest.main()
