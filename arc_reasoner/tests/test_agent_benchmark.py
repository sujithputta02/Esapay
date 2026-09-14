"""
Unit test wrapping the full ESA & ARC-AGI benchmark suite.
"""

import unittest
from arc_reasoner.agent_benchmark import run_full_benchmark


class TestAgentBenchmark(unittest.TestCase):

    def test_all_benchmark_levels(self):
        success = run_full_benchmark()
        self.assertTrue(success, "Benchmark suite must achieve 100% pass rate across all 4 levels")


if __name__ == "__main__":
    unittest.main()
