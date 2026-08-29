import unittest

import cuppingnotes


class TestCuppingnotes(unittest.TestCase):
    def test_is_within_range(self):
        self.assertEqual(cuppingnotes.is_within_range(0, True), True)


if __name__ == "__main__":
    unittest.main()
