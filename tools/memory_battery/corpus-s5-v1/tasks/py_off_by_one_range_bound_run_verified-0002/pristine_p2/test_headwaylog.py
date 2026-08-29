import unittest

import headwaylog


class TestHeadwaylog(unittest.TestCase):
    def test_schedule_slot_checkpoints(self):
        self.assertEqual(headwaylog.schedule_slot_checkpoints(3), ['cycle 1', 'cycle 2'])


if __name__ == "__main__":
    unittest.main()
