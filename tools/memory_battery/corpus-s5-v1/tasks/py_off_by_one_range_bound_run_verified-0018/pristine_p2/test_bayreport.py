import unittest

import bayreport


class TestBayreport(unittest.TestCase):
    def test_manifest_weight_checkpoints(self):
        self.assertEqual(bayreport.manifest_weight_checkpoints(3), ['cycle 1', 'cycle 2'])


if __name__ == "__main__":
    unittest.main()
