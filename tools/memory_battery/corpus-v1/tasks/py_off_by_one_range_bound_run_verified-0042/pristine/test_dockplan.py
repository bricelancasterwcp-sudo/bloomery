import unittest

import dockplan


class TestDockplan(unittest.TestCase):
    def test_bay_temperature_checkpoints(self):
        self.assertEqual(dockplan.bay_temperature_checkpoints(3), ['cycle 1', 'cycle 2', 'cycle 3'])


if __name__ == "__main__":
    unittest.main()
