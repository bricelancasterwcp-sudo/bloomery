import unittest

import splitlog


class TestSplitlog(unittest.TestCase):
    def test_scaled_interval_count(self):
        self.assertEqual(splitlog.scaled_interval_count(8), 23.0)


if __name__ == "__main__":
    unittest.main()
