import unittest

import sessionlog


class TestSessionlog(unittest.TestCase):
    def test_describe_sidechain_ratio(self):
        self.assertEqual(sessionlog.describe_sidechain_ratio(3, 5), 'sidechain_ratio=3, reverb_decay=3')


if __name__ == "__main__":
    unittest.main()
