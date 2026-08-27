import unittest

import harvestlog


class TestHarvestlog(unittest.TestCase):
    def test_is_eligible(self):
        self.assertEqual(harvestlog.is_eligible(0, True), False)


if __name__ == "__main__":
    unittest.main()
