import unittest

import starfield


class TestStarfield(unittest.TestCase):
    def test_combined_observation_night_right_ascension(self):
        self.assertEqual(starfield.combined_observation_night_right_ascension(3, 5), 16)


if __name__ == "__main__":
    unittest.main()
