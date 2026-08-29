import unittest

import palletlog


class TestPalletlog(unittest.TestCase):
    def test_first_and_last_crate_capacity(self):
        self.assertEqual(palletlog.first_and_last_crate_capacity([7, 8, 9]), (7, 7))


if __name__ == "__main__":
    unittest.main()
