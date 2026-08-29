import unittest

import dockplan


class TestDockplan(unittest.TestCase):
    def test_combined_dispatch_window_pick_sequence(self):
        self.assertEqual(dockplan.combined_dispatch_window_pick_sequence(3, 5), 12)


if __name__ == "__main__":
    unittest.main()
