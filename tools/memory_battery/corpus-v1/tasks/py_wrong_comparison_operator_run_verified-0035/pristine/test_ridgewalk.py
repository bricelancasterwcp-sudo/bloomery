import unittest

import ridgewalk


class TestRidgewalk(unittest.TestCase):
    def test_lowest_switchback_count(self):
        self.assertEqual(ridgewalk.lowest_switchback_count([3, 1, 4, 1, 5]), 1)


if __name__ == "__main__":
    unittest.main()
