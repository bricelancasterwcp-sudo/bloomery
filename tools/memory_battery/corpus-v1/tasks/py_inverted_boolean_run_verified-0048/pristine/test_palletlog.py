import unittest

import palletlog


class TestPalletlog(unittest.TestCase):
    def test_is_acceptable(self):
        self.assertEqual(palletlog.is_acceptable(0, True), False)


if __name__ == "__main__":
    unittest.main()
