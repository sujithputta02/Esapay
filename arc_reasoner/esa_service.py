"""
ESA Fluid Reasoner Microservice.
Bridges ARC-AGI-3 topological rule induction with the ESA payment gateway architecture.
Exposes a lightweight, zero-dependency HTTP interface on port 5005.
"""

import http.server
import json
import os
import sys
from typing import Any, Dict, List, Optional, Tuple
from arc_reasoner.priors.grid import Grid, to_grid


def conditions_to_health_grid(conditions: List[Dict[str, Any]]) -> Tuple[Grid, List[str]]:
    """
    Transforms ESA Condition metrics into a 2D topological health grid.
    Rows: Unique Workloads / Gateways
    Columns: [ErrorRateSeverity (0-3), LatencySeverity (0-3), QueueSeverity (0-3)]
    """
    # Group conditions by workload
    workloads: List[str] = []
    workload_metrics: Dict[str, Dict[str, float]] = {}

    for c in conditions:
        wid = c.get("workload_id", "default")
        if wid not in workload_metrics:
            workloads.append(wid)
            workload_metrics[wid] = {"error_rate": 0.0, "latency": 0.0, "queue": 0.0}

        # Inspect metric values or conditions
        m_type = c.get("condition_type", "")
        sev = c.get("severity", "")
        # Parse metrics if present
        metrics = c.get("metrics", {})
        if metrics:
            workload_metrics[wid]["error_rate"] = max(workload_metrics[wid]["error_rate"], metrics.get("error_rate", 0.0))
            workload_metrics[wid]["latency"] = max(workload_metrics[wid]["latency"], metrics.get("p95_latency_ms", 0.0))
            workload_metrics[wid]["queue"] = max(workload_metrics[wid]["queue"], float(metrics.get("queue_depth", 0)))
        else:
            # Fallback from severity / type
            sev_val = 3.0 if sev == "Critical" else (2.0 if sev == "Warning" else 1.0)
            if "Latency" in m_type or "p95" in str(c):
                workload_metrics[wid]["latency"] = max(workload_metrics[wid]["latency"], sev_val * 150.0)
            if "Error" in m_type or "error" in str(c):
                workload_metrics[wid]["error_rate"] = max(workload_metrics[wid]["error_rate"], sev_val * 0.05)
            if "Queue" in m_type or "queue" in str(c):
                workload_metrics[wid]["queue"] = max(workload_metrics[wid]["queue"], sev_val * 400.0)

    # Discretize into 0-3 severity levels for topological analysis
    grid_rows = []
    for wid in workloads:
        m = workload_metrics[wid]
        # Error severity: 0 (<1%), 1 (1-5%), 2 (5-15%), 3 (>15%)
        e_sev = 3 if m["error_rate"] > 0.15 else (2 if m["error_rate"] > 0.05 else (1 if m["error_rate"] > 0.01 else 0))
        # Latency severity: 0 (<100ms), 1 (100-250ms), 2 (250-500ms), 3 (>500ms)
        l_sev = 3 if m["latency"] > 500 else (2 if m["latency"] > 250 else (1 if m["latency"] > 100 else 0))
        # Queue severity: 0 (<100), 1 (100-500), 2 (500-1000), 3 (>1000)
        q_sev = 3 if m["queue"] > 1000 else (2 if m["queue"] > 500 else (1 if m["queue"] > 100 else 0))
        grid_rows.append((e_sev, l_sev, q_sev))

    if not grid_rows:
        grid_rows.append((0, 0, 0))
        workloads.append("default")

    return to_grid(grid_rows), workloads


def induce_payment_diagnosis(conditions: List[Dict[str, Any]]) -> Dict[str, Any]:
    """
    Applies fluid rule induction on the health grid to diagnose root causes
    and recommend mitigations without relying on an external LLM.
    """
    if not conditions:
        return {
            "hypothesis": "All payment gateways operating nominally",
            "root_cause": "None",
            "confidence": 1.0,
            "evidence_refs": [],
            "recommended_action": None,
        }

    grid, workloads = conditions_to_health_grid(conditions)
    h = len(grid)

    # Calculate cluster statistics
    total_errors = sum(row[0] for row in grid)
    total_latencies = sum(row[1] for row in grid)
    total_queues = sum(row[2] for row in grid)

    max_error_row = max(range(h), key=lambda r: grid[r][0])
    max_queue_row = max(range(h), key=lambda r: grid[r][2])
    max_latency_row = max(range(h), key=lambda r: grid[r][1])

    # Rule Induction 1: Isolated Queue/Latency spike with normal peers -> HOT_PARTITION
    if total_queues > 0 and (grid[max_queue_row][2] >= 2 or grid[max_queue_row][1] >= 2):
        if h == 1 or (total_queues - grid[max_queue_row][2]) == 0:
            target_wl = workloads[max_queue_row]
            return {
                "hypothesis": f"Fluid reasoner induced isolated bottleneck in partition for workload '{target_wl}'",
                "root_cause": "HOT_PARTITION",
                "confidence": 0.94,
                "evidence_refs": ["queue_depth", "p95_latency_ms"],
                "recommended_action": "CREATE_REPLICA",
            }

    # Rule Induction 2: Distributed High Error Rate across multiple nodes -> TRAFFIC_SPIKE
    if total_errors >= 2 and h > 1 and sum(1 for row in grid if row[0] >= 1) > 1:
        return {
            "hypothesis": "Fluid reasoner detected multi-workload error propagation from upstream gateway traffic surge",
            "root_cause": "TRAFFIC_SPIKE",
            "confidence": 0.91,
            "evidence_refs": ["error_rate", "throughput_rpm"],
            "recommended_action": "SHIFT_ROUTE",
        }

    # Rule Induction 3: High Error Rate isolated to one node without massive queue -> NODE_DEGRADATION
    if grid[max_error_row][0] >= 2:
        target_wl = workloads[max_error_row]
        return {
            "hypothesis": f"Fluid reasoner isolated hardware/provider degradation on node '{target_wl}'",
            "root_cause": "NODE_DEGRADATION",
            "confidence": 0.92,
            "evidence_refs": ["error_rate"],
            "recommended_action": "SHIFT_ROUTE",
        }

    # Rule Induction 4: Generalized queue backup across all partitions -> CAPACITY_ISSUE
    if total_queues >= 2:
        return {
            "hypothesis": "Fluid reasoner detected global cluster queue saturation requiring horizontal capacity expansion",
            "root_cause": "CAPACITY_ISSUE",
            "confidence": 0.89,
            "evidence_refs": ["queue_depth"],
            "recommended_action": "CREATE_REPLICA",
        }

    # Default fallback if anomaly detected but subtle
    return {
        "hypothesis": "Subtle threshold variance detected across payment metrics",
        "root_cause": "OTHER",
        "confidence": 0.75,
        "evidence_refs": ["p95_latency_ms"],
        "recommended_action": "SHIFT_ROUTE",
    }


class FluidReasonerHTTPHandler(http.server.BaseHTTPRequestHandler):
    """HTTP Request Handler for the ESA Fluid Reasoner Service."""

    def _set_headers(self, status: int = 200, content_type: str = "application/json"):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()

    def do_OPTIONS(self):
        self._set_headers(204)

    def do_GET(self):
        if self.path in ("/health", "/"):
            payload = {
                "status": "ok",
                "service": "esa_fluid_reasoner",
                "version": "1.0.0",
                "priors": ["objectness", "topology", "grid_geometry", "induction"],
            }
            self._set_headers(200)
            self.wfile.write(json.dumps(payload).encode("utf-8"))
        else:
            self._set_headers(404)
            self.wfile.write(b'{"error": "Not Found"}')

    def do_POST(self):
        if self.path == "/diagnose":
            try:
                content_length = int(self.headers.get("Content-Length", 0))
                body = self.rfile.read(content_length)
                data = json.loads(body.decode("utf-8"))
                conditions = data.get("conditions", []) if isinstance(data, dict) else (data if isinstance(data, list) else [])

                diagnosis = induce_payment_diagnosis(conditions)
                self._set_headers(200)
                self.wfile.write(json.dumps(diagnosis).encode("utf-8"))
            except Exception as e:
                self._set_headers(500)
                err_payload = {"error": str(e), "root_cause": "OTHER", "confidence": 0.0}
                self.wfile.write(json.dumps(err_payload).encode("utf-8"))
        else:
            self._set_headers(404)
            self.wfile.write(b'{"error": "Not Found"}')

    def log_message(self, format, *args):
        # Quiet standard logging to keep terminal clean
        if os.environ.get("VERBOSE"):
            super().log_message(format, *args)


class ConcurrentThreadingHTTPServer(http.server.ThreadingHTTPServer):
    """Multi-threaded HTTP server with expanded TCP backlog queue for high-concurrency IPC."""
    request_queue_size = 256
    daemon_threads = True


def run_server(port: int = 5005):
    """Starts the Fluid Reasoner Threaded HTTP service."""
    server_address = ("", port)
    httpd = ConcurrentThreadingHTTPServer(server_address, FluidReasonerHTTPHandler)
    print(f"🚀 ESA Fluid Reasoner Service (Multi-Threaded) listening on http://0.0.0.0:{port}")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nStopping Fluid Reasoner Service...")
        httpd.server_close()


if __name__ == "__main__":
    port = int(os.environ.get("FLUID_REASONER_PORT", 5005))
    run_server(port)
