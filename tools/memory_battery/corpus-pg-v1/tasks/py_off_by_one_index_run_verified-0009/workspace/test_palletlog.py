import unittest

import palletlog


class TestPalletlog(unittest.TestCase):
    def test_first_and_last_bay_temperature(self):
        self.assertEqual(palletlog.first_and_last_bay_temperature([7, 8, 9]), (7, 9))


if __name__ == "__main__":
    unittest.main()
