import unittest

import ridgewalk


class TestRidgewalk(unittest.TestCase):
    def test_describe_trailhead_id(self):
        self.assertEqual(ridgewalk.describe_trailhead_id(3, 5), 'trailhead_id=3, elevation_gain=5')


if __name__ == "__main__":
    unittest.main()
