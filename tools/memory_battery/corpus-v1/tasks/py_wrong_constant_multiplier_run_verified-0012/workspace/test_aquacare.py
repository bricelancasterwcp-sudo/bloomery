import unittest

import aquacare


class TestAquacare(unittest.TestCase):
    def test_scaled_salinity(self):
        self.assertEqual(aquacare.scaled_salinity(8), 12.0)


if __name__ == "__main__":
    unittest.main()
