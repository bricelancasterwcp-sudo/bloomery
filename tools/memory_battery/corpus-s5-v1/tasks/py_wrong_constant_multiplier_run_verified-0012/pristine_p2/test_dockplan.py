import unittest

import dockplan


class TestDockplan(unittest.TestCase):
    def test_scaled_conveyor_speed(self):
        self.assertEqual(dockplan.scaled_conveyor_speed(8), 23.0)


if __name__ == "__main__":
    unittest.main()
