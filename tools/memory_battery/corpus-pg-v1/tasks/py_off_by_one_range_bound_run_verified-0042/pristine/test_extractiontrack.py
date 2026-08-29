import unittest

import extractiontrack


class TestExtractiontrack(unittest.TestCase):
    def test_water_temp_c_checkpoints(self):
        self.assertEqual(extractiontrack.water_temp_c_checkpoints(3), ['cycle 1', 'cycle 2', 'cycle 3'])


if __name__ == "__main__":
    unittest.main()
