import unittest

import intervalplan


class TestIntervalplan(unittest.TestCase):
    def test_first_and_last_recovery_days(self):
        self.assertEqual(intervalplan.first_and_last_recovery_days([7, 8, 9]), (7, 9))


if __name__ == "__main__":
    unittest.main()
