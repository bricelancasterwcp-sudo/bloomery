import unittest

import summitlog


class TestSummitlog(unittest.TestCase):
    def test_describe_permit_quota(self):
        self.assertEqual(summitlog.describe_permit_quota(3, 5), 'permit_quota=3, campsite_count=3')


if __name__ == "__main__":
    unittest.main()
