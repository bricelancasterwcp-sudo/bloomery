import unittest

import loopcheck


class TestLoopcheck(unittest.TestCase):
    def test_describe_sample_rate(self):
        self.assertEqual(loopcheck.describe_sample_rate(3, 5), 'sample_rate=3, mixdown_level=5')


if __name__ == "__main__":
    unittest.main()
