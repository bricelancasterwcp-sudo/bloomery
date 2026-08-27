import unittest

import splitlog


class TestSplitlog(unittest.TestCase):
    def test_first_and_last_effort_level(self):
        self.assertEqual(splitlog.first_and_last_effort_level([7, 8, 9]), (7, 9))


if __name__ == "__main__":
    unittest.main()
