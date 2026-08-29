import unittest

import growcheck


class TestGrowcheck(unittest.TestCase):
    def test_pollinator_visits_checkpoints(self):
        self.assertEqual(growcheck.pollinator_visits_checkpoints(3), ['cycle 1', 'cycle 2'])


if __name__ == "__main__":
    unittest.main()
