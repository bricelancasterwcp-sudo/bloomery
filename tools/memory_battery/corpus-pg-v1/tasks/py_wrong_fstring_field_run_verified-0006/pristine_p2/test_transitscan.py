import unittest

import transitscan


class TestTransitscan(unittest.TestCase):
    def test_describe_telescope_id(self):
        self.assertEqual(transitscan.describe_telescope_id(3, 5), 'telescope_id=3, exposure_seconds=3')


if __name__ == "__main__":
    unittest.main()
