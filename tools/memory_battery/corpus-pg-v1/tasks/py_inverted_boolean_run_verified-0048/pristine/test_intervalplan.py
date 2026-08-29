import unittest

import intervalplan


class TestIntervalplan(unittest.TestCase):
    def test_is_ready(self):
        self.assertEqual(intervalplan.is_ready(0, True), False)


if __name__ == "__main__":
    unittest.main()
