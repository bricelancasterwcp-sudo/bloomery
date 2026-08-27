import unittest

import aquacare


class TestAquacare(unittest.TestCase):
    def test_lowest_aeration_rate(self):
        self.assertEqual(aquacare.lowest_aeration_rate([3, 1, 4, 1, 5]), 1)


if __name__ == "__main__":
    unittest.main()
