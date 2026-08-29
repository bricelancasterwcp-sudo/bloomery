import unittest

import dockplan


class TestDockplan(unittest.TestCase):
    def test_lowest_conveyor_speed(self):
        self.assertEqual(dockplan.lowest_conveyor_speed([3, 1, 4, 1, 5]), 1)


if __name__ == "__main__":
    unittest.main()
