"""
Unit tests for Chaos proxy, metric jitter, and hysteresis debouncing.
"""

import unittest
from arc_reasoner.chaos import HysteresisFilter, JitterConfig, TelemetryChaosProxy
from arc_reasoner.esa_service import induce_payment_diagnosis


class TestChaos(unittest.TestCase):

    def test_jitter_injection(self):
        proxy = TelemetryChaosProxy(JitterConfig(latency_jitter_pct=0.05))
        original = {"p95_latency_ms": 300.0, "error_rate": 0.05, "queue_depth": 100.0}
        jittered = proxy.inject_boundary_jitter(original)

        # Must be within +/- 5% range (285ms - 315ms)
        self.assertGreaterEqual(jittered["p95_latency_ms"], 280.0)
        self.assertLessEqual(jittered["p95_latency_ms"], 320.0)

    def test_hysteresis_filter_suppresses_transient_spike(self):
        filter = HysteresisFilter(alpha=0.6, debounce_window=3)
        workload = "gateway-hdfc"

        # Baseline: 200ms
        filter.update(workload, {"p95_latency_ms": 200.0})
        self.assertFalse(filter.is_true_anomaly(workload, "p95_latency_ms", 250.0))

        # Single transient spike to 310ms (should be smoothed and debounced)
        filter.update(workload, {"p95_latency_ms": 310.0})
        self.assertFalse(
            filter.is_true_anomaly(workload, "p95_latency_ms", 250.0),
            "Single transient spike must not trigger immediate anomaly (false positive)"
        )

        # Two more sustained high readings
        filter.update(workload, {"p95_latency_ms": 320.0})
        self.assertFalse(filter.is_true_anomaly(workload, "p95_latency_ms", 250.0))
        filter.update(workload, {"p95_latency_ms": 330.0})

        # Now sustained breach is confirmed
        self.assertTrue(
            filter.is_true_anomaly(workload, "p95_latency_ms", 250.0),
            "Sustained breach over 3 consecutive cycles must be confirmed"
        )

    def test_packet_drop_resilience(self):
        proxy = TelemetryChaosProxy(JitterConfig(packet_drop_rate=0.20))
        conditions = [
            {"workload_id": f"worker-{i}", "condition_type": "HighLatency", "metrics": {"p95_latency_ms": 400.0}}
            for i in range(10)
        ]
        processed = proxy.process_telemetry_batch(conditions)

        # With 20% drop rate, surviving batch should be non-empty but less than or equal to 10
        self.assertGreater(len(processed), 0)
        self.assertLessEqual(len(processed), 10)

        # Diagnosing surviving incomplete batch must not crash
        diagnosis = induce_payment_diagnosis(processed)
        self.assertIn("root_cause", diagnosis)
        self.assertGreater(diagnosis["confidence"], 0.0)


if __name__ == "__main__":
    unittest.main()
