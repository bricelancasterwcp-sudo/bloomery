import unittest

import skyledger


class TestSkyledger(unittest.TestCase):
    def test_telescope_id_value(self):
        self.assertEqual(skyledger.telescope_id_value({'primary': 1, 'secondary': 2, 'fallback': 3, 'override': 4, 'baseline': 5, 'candidate': 6, 'nominal': 7, 'adjusted': 8}), 1)


if __name__ == "__main__":
    unittest.main()
