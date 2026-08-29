import unittest

import starfield


class TestStarfield(unittest.TestCase):
    def test_highest_telescope_id(self):
        self.assertEqual(starfield.highest_telescope_id([3, 1, 4, 1, 5]), 12)


if __name__ == "__main__":
    unittest.main()
