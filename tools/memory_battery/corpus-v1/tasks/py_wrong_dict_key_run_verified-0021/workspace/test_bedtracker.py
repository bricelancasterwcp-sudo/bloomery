import unittest

import bedtracker


class TestBedtracker(unittest.TestCase):
    def test_seedling_count_value(self):
        self.assertEqual(bedtracker.seedling_count_value({'primary': 1, 'secondary': 2, 'fallback': 3, 'override': 4, 'baseline': 5, 'candidate': 6, 'nominal': 7, 'adjusted': 8}), 4)


if __name__ == "__main__":
    unittest.main()
