import unittest

import palletlog


class TestPalletlog(unittest.TestCase):
    def test_scaled_conveyor_speed(self):
        self.assertEqual(palletlog.scaled_conveyor_speed(8), 10.0)


if __name__ == "__main__":
    unittest.main()
