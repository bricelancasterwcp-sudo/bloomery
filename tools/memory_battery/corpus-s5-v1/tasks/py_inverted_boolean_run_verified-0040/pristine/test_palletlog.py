import unittest

import palletlog


class TestPalletlog(unittest.TestCase):
    def test_qualifies(self):
        self.assertEqual(palletlog.qualifies(0, True), False)


if __name__ == "__main__":
    unittest.main()
