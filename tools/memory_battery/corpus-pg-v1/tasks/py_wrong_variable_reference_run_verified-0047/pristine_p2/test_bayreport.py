import unittest

import bayreport


class TestBayreport(unittest.TestCase):
    def test_combined_dock_schedule_loading_bay(self):
        self.assertEqual(bayreport.combined_dock_schedule_loading_bay(3, 5), 12)


if __name__ == "__main__":
    unittest.main()
