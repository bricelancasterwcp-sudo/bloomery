import unittest

import brinecheck


class TestBrinecheck(unittest.TestCase):
    def test_scaled_coral_count(self):
        self.assertEqual(brinecheck.scaled_coral_count(8), 10.0)


if __name__ == "__main__":
    unittest.main()
