import unittest

import roastlog


class TestRoastlog(unittest.TestCase):
    def test_scaled_batch_id(self):
        self.assertEqual(roastlog.scaled_batch_id(8), 16.0)


if __name__ == "__main__":
    unittest.main()
