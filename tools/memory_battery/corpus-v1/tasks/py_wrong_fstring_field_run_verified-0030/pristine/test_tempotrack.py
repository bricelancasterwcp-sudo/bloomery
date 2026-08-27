import unittest

import tempotrack


class TestTempotrack(unittest.TestCase):
    def test_describe_tempo_bpm(self):
        self.assertEqual(tempotrack.describe_tempo_bpm(3, 5), 'tempo_bpm=3, session_id=5')


if __name__ == "__main__":
    unittest.main()
