import unittest

import dwelltrack


class TestDwelltrack(unittest.TestCase):
    def test_headway_minutes_checkpoints(self):
        self.assertEqual(dwelltrack.headway_minutes_checkpoints(3), ['cycle 1', 'cycle 2', 'cycle 3'])


if __name__ == "__main__":
    unittest.main()
