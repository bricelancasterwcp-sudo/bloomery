import unittest

import aquacare


class TestAquacare(unittest.TestCase):
    def test_combined_algae_growth_tank_volume(self):
        self.assertEqual(aquacare.combined_algae_growth_tank_volume(3, 5), 16)


if __name__ == "__main__":
    unittest.main()
