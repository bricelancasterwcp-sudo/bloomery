import unittest

import tanktrack


class TestTanktrack(unittest.TestCase):
    def test_combined_species_count_ph_level(self):
        self.assertEqual(tanktrack.combined_species_count_ph_level(3, 5), 16)


if __name__ == "__main__":
    unittest.main()
