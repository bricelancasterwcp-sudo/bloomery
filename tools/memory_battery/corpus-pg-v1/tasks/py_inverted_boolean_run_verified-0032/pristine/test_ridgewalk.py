import unittest

import ridgewalk


class TestRidgewalk(unittest.TestCase):
    def test_qualifies(self):
        self.assertEqual(ridgewalk.qualifies(0, True), True)


if __name__ == "__main__":
    unittest.main()
