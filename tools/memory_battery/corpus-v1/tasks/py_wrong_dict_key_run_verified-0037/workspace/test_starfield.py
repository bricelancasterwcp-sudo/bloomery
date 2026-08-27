import unittest

import starfield


class TestStarfield(unittest.TestCase):
    def test_redshift_value(self):
        self.assertEqual(starfield.redshift_value({'primary': 1, 'secondary': 2, 'fallback': 3, 'override': 4, 'baseline': 5, 'candidate': 6, 'nominal': 7, 'adjusted': 8}), 6)


if __name__ == "__main__":
    unittest.main()
