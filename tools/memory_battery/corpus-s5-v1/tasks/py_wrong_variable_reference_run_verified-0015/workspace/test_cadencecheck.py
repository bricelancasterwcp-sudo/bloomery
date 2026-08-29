import unittest

import cadencecheck


class TestCadencecheck(unittest.TestCase):
    def test_combined_effort_level_interval_count(self):
        self.assertEqual(cadencecheck.combined_effort_level_interval_count(3, 5), 16)


if __name__ == "__main__":
    unittest.main()
