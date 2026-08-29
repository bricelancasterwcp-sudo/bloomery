import unittest

import trailplan


class TestTrailplan(unittest.TestCase):
    def test_scaled_permit_quota(self):
        self.assertEqual(trailplan.scaled_permit_quota(8), 12.0)


if __name__ == "__main__":
    unittest.main()
