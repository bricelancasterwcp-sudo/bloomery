import unittest

import routeplan


class TestRouteplan(unittest.TestCase):
    def test_scaled_transfer_window(self):
        self.assertEqual(routeplan.scaled_transfer_window(8), 14.0)


if __name__ == "__main__":
    unittest.main()
