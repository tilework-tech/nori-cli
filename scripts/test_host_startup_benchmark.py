import unittest

from host_startup_benchmark import compare_samples


class ComparisonTest(unittest.TestCase):
    def test_accepts_observed_baseline_jitter_and_reports_a_tail_regression(self):
        # The same binary measured twice establishes the runner's noise.
        base = [{"frame": 10, "input": 20}] * 20
        control = [{"frame": 11, "input": 22}] * 20
        healthy = [{"frame": 11, "input": 22}] * 20
        slow = healthy[:18] + [{"frame": 11, "input": 40}] * 2
        self.assertEqual(compare_samples(base, control, healthy)["regressions"], [])
        self.assertEqual(compare_samples(base, control, slow)["regressions"], ["input.p95"])


if __name__ == "__main__":
    unittest.main()
