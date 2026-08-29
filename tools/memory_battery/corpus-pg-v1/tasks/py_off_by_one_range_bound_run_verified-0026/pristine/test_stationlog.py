import unittest

import stationlog


class TestStationlog(unittest.TestCase):
    def test_pressure_checkpoints(self):
        self.assertEqual(stationlog.pressure_checkpoints(3), ['cycle 1', 'cycle 2', 'cycle 3'])


if __name__ == "__main__":
    unittest.main()
