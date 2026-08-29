import unittest

import cadencecheck


class TestCadencecheck(unittest.TestCase):
    def test_describe_treadmill_speed(self):
        self.assertEqual(cadencecheck.describe_treadmill_speed(3, 5), 'treadmill_speed=3, recovery_days=5')


if __name__ == "__main__":
    unittest.main()
