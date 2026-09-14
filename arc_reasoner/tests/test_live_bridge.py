"""
End-to-end bridge test: tests HTTP communication between ESA client and Fluid Reasoner service.
"""

import json
import threading
import time
import unittest
import urllib.request
from arc_reasoner.esa_service import FluidReasonerHTTPHandler, http


class TestLiveBridge(unittest.TestCase):

    @classmethod
    def setUpClass(cls):
        # Start server on a test port
        cls.port = 5055
        cls.server = http.server.HTTPServer(("127.0.0.1", cls.port), FluidReasonerHTTPHandler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        time.sleep(0.1)

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()

    def test_live_health_endpoint(self):
        url = f"http://127.0.0.1:{self.port}/health"
        req = urllib.request.Request(url)
        with urllib.request.urlopen(req) as resp:
            self.assertEqual(resp.status, 200)
            data = json.loads(resp.read().decode("utf-8"))
            self.assertEqual(data["status"], "ok")
            self.assertEqual(data["service"], "esa_fluid_reasoner")

    def test_live_diagnosis_endpoint(self):
        url = f"http://127.0.0.1:{self.port}/diagnose"
        payload = {
            "conditions": [
                {
                    "workload_id": "payment-api-prod",
                    "condition_type": "QueueBacklog",
                    "severity": "High",
                    "metrics": {
                        "p95_latency_ms": 420.0,
                        "queue_depth": 1200,
                        "error_rate": 0.02
                    }
                }
            ]
        }
        req = urllib.request.Request(
            url,
            data=json.dumps(payload).encode("utf-8"),
            headers={"Content-Type": "application/json"}
        )
        with urllib.request.urlopen(req) as resp:
            self.assertEqual(resp.status, 200)
            data = json.loads(resp.read().decode("utf-8"))
            self.assertEqual(data["root_cause"], "HOT_PARTITION")
            self.assertEqual(data["recommended_action"], "CREATE_REPLICA")
            self.assertGreaterEqual(data["confidence"], 0.9)


if __name__ == "__main__":
    unittest.main()
