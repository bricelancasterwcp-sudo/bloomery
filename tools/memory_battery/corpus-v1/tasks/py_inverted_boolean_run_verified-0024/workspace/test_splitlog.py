import unittest

import splitlog


class TestSplitlog(unittest.TestCase):
    def test_passes_check(self):
        self.assertEqual(splitlog.passes_check(0, True), False)


if __name__ == "__main__":
    unittest.main()
