import unittest

import brinecheck


class TestBrinecheck(unittest.TestCase):
    def test_highest_tank_volume(self):
        self.assertEqual(brinecheck.highest_tank_volume([3, 1, 4, 1, 5]), 1)


if __name__ == "__main__":
    unittest.main()
