import unittest

import headwaylog


class TestHeadwaylog(unittest.TestCase):
    def test_schedule_slot_value(self):
        self.assertEqual(headwaylog.schedule_slot_value({'primary': 1, 'secondary': 2, 'fallback': 3, 'override': 4, 'baseline': 5, 'candidate': 6, 'nominal': 7, 'adjusted': 8}), 8)


if __name__ == "__main__":
    unittest.main()
