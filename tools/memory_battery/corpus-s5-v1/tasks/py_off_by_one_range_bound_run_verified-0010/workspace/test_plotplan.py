import unittest

import plotplan


class TestPlotplan(unittest.TestCase):
    def test_greenhouse_temp_checkpoints(self):
        self.assertEqual(plotplan.greenhouse_temp_checkpoints(3), ['cycle 1', 'cycle 2', 'cycle 3'])


if __name__ == "__main__":
    unittest.main()
