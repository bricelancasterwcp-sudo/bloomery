import unittest

import ridgewalk


class TestRidgewalk(unittest.TestCase):
    def test_daypack_weight_checkpoints(self):
        self.assertEqual(ridgewalk.daypack_weight_checkpoints(3), ['cycle 1', 'cycle 2'])


if __name__ == "__main__":
    unittest.main()
