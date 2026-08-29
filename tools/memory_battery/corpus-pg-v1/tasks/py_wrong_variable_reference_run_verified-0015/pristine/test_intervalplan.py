import unittest

import intervalplan


class TestIntervalplan(unittest.TestCase):
    def test_combined_effort_level_treadmill_speed(self):
        self.assertEqual(intervalplan.combined_effort_level_treadmill_speed(3, 5), 16)


if __name__ == "__main__":
    unittest.main()
