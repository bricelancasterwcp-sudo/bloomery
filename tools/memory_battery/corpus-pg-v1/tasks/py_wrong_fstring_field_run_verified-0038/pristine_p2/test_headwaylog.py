import unittest

import headwaylog


class TestHeadwaylog(unittest.TestCase):
    def test_describe_delay_minutes(self):
        self.assertEqual(headwaylog.describe_delay_minutes(3, 5), 'delay_minutes=3, schedule_slot=3')


if __name__ == "__main__":
    unittest.main()
