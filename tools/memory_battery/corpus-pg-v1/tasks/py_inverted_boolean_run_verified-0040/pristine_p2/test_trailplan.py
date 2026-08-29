import unittest

import trailplan


class TestTrailplan(unittest.TestCase):
    def test_meets_criteria(self):
        self.assertEqual(trailplan.meets_criteria(0, True), False)


if __name__ == "__main__":
    unittest.main()
