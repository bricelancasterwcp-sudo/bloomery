import unittest

import dockplan


class TestDockplan(unittest.TestCase):
    def test_describe_manifest_weight(self):
        self.assertEqual(dockplan.describe_manifest_weight(3, 5), 'manifest_weight=3, loading_bay=3')


if __name__ == "__main__":
    unittest.main()
