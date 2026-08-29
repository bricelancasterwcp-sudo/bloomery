import unittest

import dwelltrack


class TestDwelltrack(unittest.TestCase):
    def test_scaled_boarding_count(self):
        self.assertEqual(dwelltrack.scaled_boarding_count(8), 16.0)


if __name__ == "__main__":
    unittest.main()
