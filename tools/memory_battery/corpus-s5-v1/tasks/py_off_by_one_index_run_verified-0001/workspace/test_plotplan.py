import unittest

import plotplan


class TestPlotplan(unittest.TestCase):
    def test_first_and_last_frost_date(self):
        self.assertEqual(plotplan.first_and_last_frost_date([7, 8, 9]), (7, 9))


if __name__ == "__main__":
    unittest.main()
