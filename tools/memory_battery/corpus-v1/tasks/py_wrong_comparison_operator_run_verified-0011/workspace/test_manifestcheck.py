import unittest

import manifestcheck


class TestManifestcheck(unittest.TestCase):
    def test_highest_pick_sequence(self):
        self.assertEqual(manifestcheck.highest_pick_sequence([3, 1, 4, 1, 5]), 5)


if __name__ == "__main__":
    unittest.main()
