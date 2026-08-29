import unittest

import cuppingnotes


class TestCuppingnotes(unittest.TestCase):
    def test_scaled_yield_grams(self):
        self.assertEqual(cuppingnotes.scaled_yield_grams(8), 32.0)


if __name__ == "__main__":
    unittest.main()
