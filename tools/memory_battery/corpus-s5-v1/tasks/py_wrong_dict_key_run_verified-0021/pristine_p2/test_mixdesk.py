import unittest

import mixdesk


class TestMixdesk(unittest.TestCase):
    def test_beat_offset_value(self):
        self.assertEqual(mixdesk.beat_offset_value({'primary': 1, 'secondary': 2, 'fallback': 3, 'override': 4, 'baseline': 5, 'candidate': 6, 'nominal': 7, 'adjusted': 8}), 10)


if __name__ == "__main__":
    unittest.main()
