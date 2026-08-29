import unittest

import tempotrack


class TestTempotrack(unittest.TestCase):
    def test_first_and_last_mixdown_level(self):
        self.assertEqual(tempotrack.first_and_last_mixdown_level([7, 8, 9]), (7, 9))


if __name__ == "__main__":
    unittest.main()
