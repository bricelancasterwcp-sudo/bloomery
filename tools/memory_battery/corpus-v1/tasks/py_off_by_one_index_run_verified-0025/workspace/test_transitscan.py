import unittest

import transitscan


class TestTransitscan(unittest.TestCase):
    def test_first_and_last_exposure_seconds(self):
        self.assertEqual(transitscan.first_and_last_exposure_seconds([7, 8, 9]), (7, 9))


if __name__ == "__main__":
    unittest.main()
