import unittest

import headwaylog


class TestHeadwaylog(unittest.TestCase):
    def test_combined_platform_number_headway_minutes(self):
        self.assertEqual(headwaylog.combined_platform_number_headway_minutes(3, 5), 16)


if __name__ == "__main__":
    unittest.main()
