"""
Chaos & Telemetry Jitter Layer for ESA Payment Gateway.
Simulates boundary metric jitter, packet drops, and out-of-order webhooks.
Provides Exponential Moving Average (EMA) and hysteresis filters to eliminate false-positive flapping.
"""

import random
from collections import deque
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple


@dataclass
class JitterConfig:
    """Configuration for telemetry metric jitter injection."""
    latency_jitter_pct: float = 0.05      # +/- 5% jitter
    error_jitter_pct: float = 0.005       # +/- 0.5% error jitter
    queue_jitter_pct: float = 0.05        # +/- 5% queue jitter
    packet_drop_rate: float = 0.15        # 15% packet drop probability
    out_of_order_rate: float = 0.10       # 10% probability of reordering


class TelemetryChaosProxy:
    """
    Simulates real-world non-stationary network conditions:
    - Injects boundary hovering noise around sensitive decision thresholds
    - Randomly drops telemetry frames
    - Permutes arrival order of webhook metrics
    """

    def __init__(self, config: Optional[JitterConfig] = None):
        self.config = config or JitterConfig()
        self._buffer: List[Dict[str, Any]] = []

    def inject_boundary_jitter(self, metrics: Dict[str, float]) -> Dict[str, float]:
        """Adds stochastic Gaussian/Uniform noise to latency, error_rate, and queue."""
        res = dict(metrics)
        if "p95_latency_ms" in res:
            jitter = (random.uniform(-1.0, 1.0) * self.config.latency_jitter_pct) * res["p95_latency_ms"]
            res["p95_latency_ms"] = max(1.0, res["p95_latency_ms"] + jitter)

        if "error_rate" in res:
            jitter = random.uniform(-1.0, 1.0) * self.config.error_jitter_pct
            res["error_rate"] = max(0.0, min(1.0, res["error_rate"] + jitter))

        if "queue_depth" in res:
            jitter = (random.uniform(-1.0, 1.0) * self.config.queue_jitter_pct) * res["queue_depth"]
            res["queue_depth"] = max(0.0, res["queue_depth"] + jitter)

        return res

    def process_telemetry_batch(
        self,
        conditions: List[Dict[str, Any]]
    ) -> List[Dict[str, Any]]:
        """
        Applies packet drop, jitter, and out-of-order permutation to a condition stream.
        """
        surviving: List[Dict[str, Any]] = []

        for c in conditions:
            # Packet drop simulation
            if random.random() < self.config.packet_drop_rate:
                continue  # Dropped frame

            corrupted = dict(c)
            if "metrics" in corrupted and isinstance(corrupted["metrics"], dict):
                corrupted["metrics"] = self.inject_boundary_jitter(corrupted["metrics"])
            surviving.append(corrupted)

        # Out-of-order arrival simulation
        if len(surviving) > 1 and random.random() < self.config.out_of_order_rate:
            random.shuffle(surviving)

        return surviving


class HysteresisFilter:
    """
    Exponential Moving Average (EMA) and debouncing filter.
    Prevents boundary metric fluctuations (e.g. latency oscillating between 290ms and 310ms)
    from triggering rapid false-positive routing mutations.
    """

    def __init__(self, alpha: float = 0.35, debounce_window: int = 3):
        self.alpha = alpha  # Smoothing factor (0 < alpha <= 1)
        self.debounce_window = debounce_window
        self.smoothed_metrics: Dict[str, Dict[str, float]] = {}
        self.consecutive_breaches: Dict[str, int] = {}

    def update(self, workload_id: str, raw_metrics: Dict[str, float]) -> Dict[str, float]:
        """Updates smoothed EMA metrics for the workload."""
        if workload_id not in self.smoothed_metrics:
            self.smoothed_metrics[workload_id] = dict(raw_metrics)
            return dict(raw_metrics)

        current = self.smoothed_metrics[workload_id]
        for key, val in raw_metrics.items():
            if isinstance(val, (int, float)):
                old_val = current.get(key, val)
                # EMA: smoothed = alpha * raw + (1 - alpha) * old
                current[key] = (self.alpha * val) + ((1.0 - self.alpha) * old_val)

        return dict(current)

    def is_true_anomaly(self, workload_id: str, threshold_key: str, threshold_val: float) -> bool:
        """
        Requires metrics to exceed the threshold for consecutive evaluations
        before declaring an actionable anomaly (debouncing).
        """
        metrics = self.smoothed_metrics.get(workload_id, {})
        val = metrics.get(threshold_key, 0.0)

        if val > threshold_val:
            self.consecutive_breaches[workload_id] = self.consecutive_breaches.get(workload_id, 0) + 1
        else:
            self.consecutive_breaches[workload_id] = 0

        # Only confirm anomaly if sustained over the debouncing window
        return self.consecutive_breaches.get(workload_id, 0) >= self.debounce_window
