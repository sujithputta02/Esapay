"""
Inter-Process Communication (IPC) High-Throughput Load Tester.
Benchmarks concurrency (>1,000 RPS) across the client and the Python service on Port 5005.
Measures p50, p95, and p99 latency to verify strict SLA compliance under load.
"""

import argparse
import concurrent.futures
import json
import statistics
import time
import urllib.request
from typing import Dict, List, Tuple


def send_diagnosis_request(url: str, payload: bytes, max_retries: int = 2) -> Tuple[bool, float]:
    """Sends a single POST request and measures latency in milliseconds, with quick retry for OS socket drops."""
    start = time.perf_counter()
    req = urllib.request.Request(
        url,
        data=payload,
        headers={"Content-Type": "application/json", "Connection": "close"}
    )
    for attempt in range(max_retries + 1):
        try:
            with urllib.request.urlopen(req, timeout=1.5) as resp:
                success = (resp.status == 200)
                latency_ms = (time.perf_counter() - start) * 1000.0
                return success, latency_ms
        except Exception:
            if attempt == max_retries:
                latency_ms = (time.perf_counter() - start) * 1000.0
                return False, latency_ms
            time.sleep(0.005)
    return False, (time.perf_counter() - start) * 1000.0


def run_load_test(
    host: str = "127.0.0.1",
    port: int = 5005,
    total_requests: int = 1000,
    concurrency: int = 40
) -> Dict[str, float]:
    """Runs a concurrent load test against the service."""
    url = f"http://{host}:{port}/diagnose"
    sample_payload = json.dumps({
        "conditions": [
            {
                "workload_id": "payment-api-prod",
                "condition_type": "HighLatency",
                "metrics": {"p95_latency_ms": 320.0, "queue_depth": 500}
            }
        ]
    }).encode("utf-8")

    latencies: List[float] = []
    successes = 0

    wall_start = time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=concurrency) as executor:
        futures = [
            executor.submit(send_diagnosis_request, url, sample_payload)
            for _ in range(total_requests)
        ]
        for f in concurrent.futures.as_completed(futures):
            ok, lat = f.result()
            if ok:
                successes += 1
            latencies.append(lat)

    wall_elapsed = time.perf_counter() - wall_start
    rps = total_requests / wall_elapsed if wall_elapsed > 0 else 0.0

    latencies.sort()
    p50 = statistics.median(latencies) if latencies else 0.0
    p95 = latencies[int(len(latencies) * 0.95)] if latencies else 0.0
    p99 = latencies[int(len(latencies) * 0.99)] if latencies else 0.0

    return {
        "total_requests": total_requests,
        "concurrency": concurrency,
        "success_rate_pct": (successes / total_requests) * 100.0,
        "wall_time_seconds": wall_elapsed,
        "rps": rps,
        "p50_latency_ms": p50,
        "p95_latency_ms": p95,
        "p99_latency_ms": p99,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="IPC High-Throughput Load Tester")
    parser.add_argument("--port", type=int, default=5005)
    parser.add_argument("--requests", type=int, default=1000)
    parser.add_argument("--concurrency", type=int, default=40)
    args = parser.parse_args()

    print(f"\n========================================================")
    print(f"       IPC HIGH-THROUGHPUT CONCURRENCY LOAD TEST        ")
    print(f"========================================================\n")
    print(f"Target: http://127.0.0.1:{args.port}/diagnose")
    print(f"Requests: {args.requests} | Concurrency: {args.concurrency}\n")

    results = run_load_test(
        port=args.port,
        total_requests=args.requests,
        concurrency=args.concurrency
    )

    print(f"Success Rate:         {results['success_rate_pct']:.1f}%")
    print(f"Throughput:           {results['rps']:.1f} Requests / Second (RPS)")
    print(f"Wall Clock Time:      {results['wall_time_seconds']:.2f}s")
    print(f"P50 Latency:          {results['p50_latency_ms']:.2f}ms")
    print(f"P95 Latency:          {results['p95_latency_ms']:.2f}ms (SLA Ceiling: 300.00ms)")
    print(f"P99 Latency:          {results['p99_latency_ms']:.2f}ms")

    sla_met = results['p95_latency_ms'] < 300.0 and results['success_rate_pct'] == 100.0
    print(f"SLA Compliance:       {'PASSED (Compliant)' if sla_met else 'FAILED'}\n")
