import unittest

import routeplan


class TestRouteplan(unittest.TestCase):
    def test_describe_occupancy_ratio(self):
        self.assertEqual(routeplan.describe_occupancy_ratio(3, 5), 'occupancy_ratio=3, delay_minutes=5')


if __name__ == "__main__":
    unittest.main()
