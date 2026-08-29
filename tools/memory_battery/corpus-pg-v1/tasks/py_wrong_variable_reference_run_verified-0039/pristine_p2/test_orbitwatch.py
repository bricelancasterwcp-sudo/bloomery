import unittest

import orbitwatch


class TestOrbitwatch(unittest.TestCase):
    def test_combined_exposure_seconds_orbital_period(self):
        self.assertEqual(orbitwatch.combined_exposure_seconds_orbital_period(3, 5), 12)


if __name__ == "__main__":
    unittest.main()
