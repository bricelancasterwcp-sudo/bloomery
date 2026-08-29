import unittest

import summitlog


class TestSummitlog(unittest.TestCase):
    def test_first_and_last_daypack_weight(self):
        self.assertEqual(summitlog.first_and_last_daypack_weight([7, 8, 9]), (7, 7))


if __name__ == "__main__":
    unittest.main()
