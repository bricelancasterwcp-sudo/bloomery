import unittest

import roastlog


class TestRoastlog(unittest.TestCase):
    def test_bean_weight_checkpoints(self):
        self.assertEqual(roastlog.bean_weight_checkpoints(3), ['cycle 1', 'cycle 2', 'cycle 3'])


if __name__ == "__main__":
    unittest.main()
