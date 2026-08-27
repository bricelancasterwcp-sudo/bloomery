import unittest

import trailplan


class TestTrailplan(unittest.TestCase):
    def test_describe_summit_elevation(self):
        self.assertEqual(trailplan.describe_summit_elevation(3, 5), 'summit_elevation=3, trail_length=5')


if __name__ == "__main__":
    unittest.main()
