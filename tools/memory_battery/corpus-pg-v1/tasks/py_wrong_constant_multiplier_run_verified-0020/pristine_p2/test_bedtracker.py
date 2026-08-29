import unittest

import bedtracker


class TestBedtracker(unittest.TestCase):
    def test_scaled_bed_width(self):
        self.assertEqual(bedtracker.scaled_bed_width(8), 14.0)


if __name__ == "__main__":
    unittest.main()
