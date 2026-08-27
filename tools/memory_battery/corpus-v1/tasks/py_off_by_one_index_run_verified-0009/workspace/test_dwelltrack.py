import unittest

import dwelltrack


class TestDwelltrack(unittest.TestCase):
    def test_first_and_last_schedule_slot(self):
        self.assertEqual(dwelltrack.first_and_last_schedule_slot([7, 8, 9]), (7, 9))


if __name__ == "__main__":
    unittest.main()
