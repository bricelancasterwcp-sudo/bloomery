import unittest

import palletlog


class TestPalletlog(unittest.TestCase):
    def test_scaled_dock_schedule(self):
        self.assertEqual(palletlog.scaled_dock_schedule(8), 4.0)


if __name__ == "__main__":
    unittest.main()
