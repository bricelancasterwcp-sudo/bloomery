import unittest

import sessionlog


class TestSessionlog(unittest.TestCase):
    def test_first_and_last_tempo_bpm(self):
        self.assertEqual(sessionlog.first_and_last_tempo_bpm([7, 8, 9]), (7, 7))


if __name__ == "__main__":
    unittest.main()
