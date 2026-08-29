import unittest

import splitlog


class TestSplitlog(unittest.TestCase):
    def test_stride_length_checkpoints(self):
        self.assertEqual(splitlog.stride_length_checkpoints(3), ['cycle 1', 'cycle 2'])


if __name__ == "__main__":
    unittest.main()
