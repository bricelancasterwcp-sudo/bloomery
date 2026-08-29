import unittest

import routeplan


class TestRouteplan(unittest.TestCase):
    def test_is_eligible(self):
        self.assertEqual(routeplan.is_eligible(0, True), False)


if __name__ == "__main__":
    unittest.main()
