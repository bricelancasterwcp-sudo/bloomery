import unittest

import extractiontrack


class TestExtractiontrack(unittest.TestCase):
    def test_describe_roast_level(self):
        self.assertEqual(extractiontrack.describe_roast_level(3, 5), 'roast_level=3, bloom_seconds=5')


if __name__ == "__main__":
    unittest.main()
