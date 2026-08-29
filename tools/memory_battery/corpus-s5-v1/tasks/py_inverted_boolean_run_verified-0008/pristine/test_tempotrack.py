import unittest

import tempotrack


class TestTempotrack(unittest.TestCase):
    def test_passes_check(self):
        self.assertEqual(tempotrack.passes_check(0, True), True)


if __name__ == "__main__":
    unittest.main()
