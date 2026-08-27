import unittest

import roastlog


class TestRoastlog(unittest.TestCase):
    def test_first_and_last_cupping_score(self):
        self.assertEqual(roastlog.first_and_last_cupping_score([7, 8, 9]), (7, 9))


if __name__ == "__main__":
    unittest.main()
