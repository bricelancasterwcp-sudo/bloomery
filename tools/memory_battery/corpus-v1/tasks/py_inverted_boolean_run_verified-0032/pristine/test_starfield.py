import unittest

import starfield


class TestStarfield(unittest.TestCase):
    def test_passes_check(self):
        self.assertEqual(starfield.passes_check(0, True), True)


if __name__ == "__main__":
    unittest.main()
