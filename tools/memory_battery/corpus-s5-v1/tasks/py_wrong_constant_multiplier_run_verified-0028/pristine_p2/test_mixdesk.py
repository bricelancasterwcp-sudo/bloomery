import unittest

import mixdesk


class TestMixdesk(unittest.TestCase):
    def test_scaled_session_id(self):
        self.assertEqual(mixdesk.scaled_session_id(8), 4.0)


if __name__ == "__main__":
    unittest.main()
