import unittest

import extractiontrack


class TestExtractiontrack(unittest.TestCase):
    def test_first_and_last_roast_level(self):
        self.assertEqual(extractiontrack.first_and_last_roast_level([7, 8, 9]), (7, 9))


if __name__ == "__main__":
    unittest.main()
