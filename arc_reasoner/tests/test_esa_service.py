"""
Unit tests for the ESA Fluid Reasoner bridge service.
"""

import unittest
from arc_reasoner.esa_service import conditions_to_health_grid, induce_payment_diagnosis


class TestESAService(unittest.TestCase):

    def test_empty_conditions(self):
        diag = induce_payment_diagnosis([])
        self.assertEqual(diag["root_cause"], "None")
        self.assertEqual(diag["confidence"], 1.0)
        self.assertIsNone(diag["recommended_action"])

    def test_hot_partition_diagnosis(self):
        conditions = [
            {
                "workload_id": "payment-api-prod",
                "condition_type": "HighLatency",
                "severity": "Critical",
                "metrics": {
                    "p95_latency_ms": 320.0,
                    "queue_depth": 850,
                    "error_rate": 0.02,
                }
            }
        ]
        diag = induce_payment_diagnosis(conditions)
        self.assertEqual(diag["root_cause"], "HOT_PARTITION")
        self.assertEqual(diag["recommended_action"], "CREATE_REPLICA")
        self.assertGreaterEqual(diag["confidence"], 0.9)

    def test_node_degradation_diagnosis(self):
        conditions = [
            {
                "workload_id": "payment-api-india-south",
                "condition_type": "HighErrorRate",
                "severity": "Critical",
                "metrics": {
                    "p95_latency_ms": 90.0,
                    "queue_depth": 20,
                    "error_rate": 0.18,
                }
            }
        ]
        diag = induce_payment_diagnosis(conditions)
        self.assertEqual(diag["root_cause"], "NODE_DEGRADATION")
        self.assertEqual(diag["recommended_action"], "SHIFT_ROUTE")
        self.assertGreaterEqual(diag["confidence"], 0.9)

    def test_traffic_spike_diagnosis(self):
        conditions = [
            {
                "workload_id": "gateway-hdfc",
                "condition_type": "HighErrorRate",
                "severity": "Warning",
                "metrics": {"error_rate": 0.08, "p95_latency_ms": 150.0, "queue_depth": 50}
            },
            {
                "workload_id": "gateway-icici",
                "condition_type": "HighErrorRate",
                "severity": "Warning",
                "metrics": {"error_rate": 0.09, "p95_latency_ms": 180.0, "queue_depth": 60}
            }
        ]
        diag = induce_payment_diagnosis(conditions)
        self.assertEqual(diag["root_cause"], "TRAFFIC_SPIKE")
        self.assertEqual(diag["recommended_action"], "SHIFT_ROUTE")


if __name__ == "__main__":
    unittest.main()
