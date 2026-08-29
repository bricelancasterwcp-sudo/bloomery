import unittest

import climatetrend


class TestClimatetrend(unittest.TestCase):
    def test_qualifies(self):
        self.assertEqual(climatetrend.qualifies(0, True), True)


if __name__ == "__main__":
    unittest.main()
