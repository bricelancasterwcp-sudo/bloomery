import unittest

import ridgewalk


class TestRidgewalk(unittest.TestCase):
    def test_trailhead_id_value(self):
        self.assertEqual(ridgewalk.trailhead_id_value({'primary': 1, 'secondary': 2, 'fallback': 3, 'override': 4, 'baseline': 5, 'candidate': 6, 'nominal': 7, 'adjusted': 8}), 5)


if __name__ == "__main__":
    unittest.main()
