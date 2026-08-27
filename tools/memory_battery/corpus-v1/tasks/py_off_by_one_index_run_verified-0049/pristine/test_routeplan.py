import unittest

import routeplan


class TestRouteplan(unittest.TestCase):
    def test_first_and_last_occupancy_ratio(self):
        self.assertEqual(routeplan.first_and_last_occupancy_ratio([7, 8, 9]), (7, 9))


if __name__ == "__main__":
    unittest.main()
