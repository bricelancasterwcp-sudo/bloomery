import unittest

import aquacare


class TestAquacare(unittest.TestCase):
    def test_describe_quarantine_days(self):
        self.assertEqual(aquacare.describe_quarantine_days(3, 5), 'quarantine_days=3, salinity=5')


if __name__ == "__main__":
    unittest.main()
