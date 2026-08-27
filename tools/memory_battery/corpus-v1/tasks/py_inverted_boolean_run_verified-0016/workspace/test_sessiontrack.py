import unittest

import sessiontrack


class TestSessiontrack(unittest.TestCase):
    def test_meets_criteria(self):
        self.assertEqual(sessiontrack.meets_criteria(0, True), False)


if __name__ == "__main__":
    unittest.main()
