import unittest

import dwelltrack


class TestDwelltrack(unittest.TestCase):
    def test_passes_check(self):
        self.assertEqual(dwelltrack.passes_check(0, True), True)


if __name__ == "__main__":
    unittest.main()
