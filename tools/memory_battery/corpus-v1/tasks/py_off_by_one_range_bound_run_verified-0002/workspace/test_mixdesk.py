import unittest

import mixdesk


class TestMixdesk(unittest.TestCase):
    def test_sample_rate_checkpoints(self):
        self.assertEqual(mixdesk.sample_rate_checkpoints(3), ['cycle 1', 'cycle 2', 'cycle 3'])


if __name__ == "__main__":
    unittest.main()
