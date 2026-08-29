import unittest

import aquacare


class TestAquacare(unittest.TestCase):
    def test_is_ready(self):
        self.assertEqual(aquacare.is_ready(0, True), True)


if __name__ == "__main__":
    unittest.main()
