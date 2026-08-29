import unittest

import growcheck


class TestGrowcheck(unittest.TestCase):
    def test_seedling_count_checkpoints(self):
        self.assertEqual(growcheck.seedling_count_checkpoints(3), ['cycle 1', 'cycle 2', 'cycle 3'])


if __name__ == "__main__":
    unittest.main()
