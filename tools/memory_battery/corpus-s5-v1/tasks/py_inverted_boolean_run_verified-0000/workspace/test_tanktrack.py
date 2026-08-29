import unittest

import tanktrack


class TestTanktrack(unittest.TestCase):
    def test_is_eligible(self):
        self.assertEqual(tanktrack.is_eligible(0, True), True)


if __name__ == "__main__":
    unittest.main()
