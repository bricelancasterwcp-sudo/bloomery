import unittest

import sessiontrack


class TestSessiontrack(unittest.TestCase):
    def test_is_ready(self):
        self.assertEqual(sessiontrack.is_ready(0, True), False)


if __name__ == "__main__":
    unittest.main()
