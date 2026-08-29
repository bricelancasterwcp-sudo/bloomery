import unittest

import headwaylog


class TestHeadwaylog(unittest.TestCase):
    def test_is_acceptable(self):
        self.assertEqual(headwaylog.is_acceptable(0, True), True)


if __name__ == "__main__":
    unittest.main()
