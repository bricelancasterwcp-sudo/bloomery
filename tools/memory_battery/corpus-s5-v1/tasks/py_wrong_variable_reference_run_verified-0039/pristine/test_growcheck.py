import unittest

import growcheck


class TestGrowcheck(unittest.TestCase):
    def test_combined_mulch_depth_frost_date(self):
        self.assertEqual(growcheck.combined_mulch_depth_frost_date(3, 5), 16)


if __name__ == "__main__":
    unittest.main()
