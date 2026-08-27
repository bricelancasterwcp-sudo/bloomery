import unittest

import summitlog


class TestSummitlog(unittest.TestCase):
    def test_permit_quota_value(self):
        self.assertEqual(summitlog.permit_quota_value({'primary': 1, 'secondary': 2, 'fallback': 3, 'override': 4, 'baseline': 5, 'candidate': 6, 'nominal': 7, 'adjusted': 8}), 7)


if __name__ == "__main__":
    unittest.main()
