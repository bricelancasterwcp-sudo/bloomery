import unittest

import aquacare


class TestAquacare(unittest.TestCase):
    def test_qualifies(self):
        self.assertEqual(aquacare.qualifies(0, True), False)


if __name__ == "__main__":
    unittest.main()
