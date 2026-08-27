import unittest

import extractiontrack


class TestExtractiontrack(unittest.TestCase):
    def test_highest_tamp_pressure(self):
        self.assertEqual(extractiontrack.highest_tamp_pressure([3, 1, 4, 1, 5]), 5)


if __name__ == "__main__":
    unittest.main()
