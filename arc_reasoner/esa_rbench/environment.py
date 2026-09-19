"""
ESA-RBench Interactive Payment Environment.
Models non-stationary traffic, downstream bank rail degradation, delayed pod startup,
thundering herd cascades, synthetic health probes, OCC versioning, and telemetry corruption.
"""

from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple
import copy
import math
import random


@dataclass
class PaymentRoute:
    name: str
    capacity_rps: float
    current_traffic_rps: float = 100.0
    base_traffic_rps: float = 100.0
    latency_ms: float = 85.0
    error_rate: float = 0.002
    queue_depth: int = 15
    pod_replicas: int = 3
    base_replicas: int = 3
    target_replicas: int = 3
    replica_warmup_steps: int = 0
    upstream_bank_health: float = 1.0  # 1.0 = nominal, 0.0 = total outage
    synthetic_probe_error: float = 0.002
    last_action_step: int = -100


@dataclass
class EnvironmentStateSnapshot:
    step: int
    state_version: int
    routes: Dict[str, PaymentRoute]
    p95_latency_ms: float
    cluster_error_rate: float
    total_queue_depth: int


class PaymentEnvironment:
    """
    Interactive rollout environment for payment infrastructure resilience.
    Simulates real-world payment rail physics, multi-signal dependencies, and delayed consequences.
    """

    def __init__(self, seed: int = 481923, topology_shift: bool = False):
        self.rng = random.Random(seed)
        self.seed = seed
        self.step_idx = 0
        self.state_version = 1
        self.snapshots: List[EnvironmentStateSnapshot] = []
        self.history: List[Dict[str, Any]] = []

        # Standard routes
        self.routes: Dict[str, PaymentRoute] = {
            "Razorpay_UPI": PaymentRoute(name="Razorpay_UPI", capacity_rps=500.0, current_traffic_rps=200.0, base_traffic_rps=200.0, pod_replicas=3, base_replicas=3),
            "HDFC_Card": PaymentRoute(name="HDFC_Card", capacity_rps=400.0, current_traffic_rps=150.0, base_traffic_rps=150.0, pod_replicas=3, base_replicas=3),
            "ICICI_Netbanking": PaymentRoute(name="ICICI_Netbanking", capacity_rps=350.0, current_traffic_rps=120.0, base_traffic_rps=120.0, pod_replicas=3, base_replicas=3),
            "SBI_UPI": PaymentRoute(name="SBI_UPI", capacity_rps=450.0, current_traffic_rps=180.0, base_traffic_rps=180.0, pod_replicas=3, base_replicas=3),
        }

        # Dynamic topology shift: introduce unseen regional/partner rails
        if topology_shift:
            partner_pools = [
                ("Axis_Credit", 300.0, 80.0, 2),
                ("Federal_Direct", 200.0, 50.0, 2),
                ("Kotak_Gateway", 260.0, 70.0, 2),
                ("YesBank_IMPS", 220.0, 60.0, 2),
            ]
            # Select 2 partners deterministically based on seed
            p1 = partner_pools[self.seed % len(partner_pools)]
            p2 = partner_pools[(self.seed + 1) % len(partner_pools)]
            self.routes[p1[0]] = PaymentRoute(name=p1[0], capacity_rps=p1[1], current_traffic_rps=p1[2], base_traffic_rps=p1[2], pod_replicas=p1[3], base_replicas=p1[3])
            self.routes[p2[0]] = PaymentRoute(name=p2[0], capacity_rps=p2[1], current_traffic_rps=p2[2], base_traffic_rps=p2[2], pod_replicas=p2[3], base_replicas=p2[3])

        # Global aggregate vitals
        self.p95_latency_ms: float = 95.0
        self.cluster_error_rate: float = 0.002
        self.total_queue_depth: int = 50

        # Fault injections
        self.active_faults: List[Dict[str, Any]] = []
        self.telemetry_corruption: bool = False
        self.adversarial_injection: Optional[str] = None

    def capture_snapshot(self) -> int:
        """Captures pre-mutation numeric state snapshot for potential rollback."""
        snapshot = EnvironmentStateSnapshot(
            step=self.step_idx,
            state_version=self.state_version,
            routes=copy.deepcopy(self.routes),
            p95_latency_ms=self.p95_latency_ms,
            cluster_error_rate=self.cluster_error_rate,
            total_queue_depth=self.total_queue_depth,
        )
        self.snapshots.append(snapshot)
        return len(self.snapshots) - 1

    def rollback_to_snapshot(self, snapshot_idx: int) -> bool:
        """Restores state to a previously captured snapshot."""
        if 0 <= snapshot_idx < len(self.snapshots):
            snap = self.snapshots[snapshot_idx]
            self.routes = copy.deepcopy(snap.routes)
            self.p95_latency_ms = snap.p95_latency_ms
            self.cluster_error_rate = snap.cluster_error_rate
            self.total_queue_depth = snap.total_queue_depth
            self.state_version += 1
            return True
        return False

    def inject_fault(self, fault_type: str, target: str, magnitude: float = 1.0, duration: int = 15):
        """Injects a physical infrastructure or provider rail fault."""
        self.active_faults.append({
            "type": fault_type,
            "target": target,
            "magnitude": magnitude,
            "start_step": self.step_idx,
            "duration": duration,
        })

    def get_telemetry_observation(self) -> Dict[str, Any]:
        """
        Returns the current telemetry observation.
        If telemetry corruption is enabled, injects sensor dropouts or contradictory noise.
        """
        obs_routes = {}
        for name, r in self.routes.items():
            err = r.error_rate
            lat = r.latency_ms
            q = r.queue_depth

            if self.telemetry_corruption and self.rng.random() < 0.35:
                # Corrupt telemetry with NaN, extreme noise, or frozen counters
                err = -1.0 if self.rng.random() < 0.5 else 0.999
                lat = 9999.0 if self.rng.random() < 0.5 else 0.1

            obs_routes[name] = {
                "route": name,
                "latency_ms": lat,
                "error_rate": err,
                "queue_depth": q,
                "pod_replicas": r.pod_replicas,
                "traffic_rps": r.current_traffic_rps,
                "capacity_rps": r.capacity_rps,
                "synthetic_probe_error": r.synthetic_probe_error,
            }

        return {
            "step": self.step_idx,
            "state_version": self.state_version,
            "p95_latency_ms": self.p95_latency_ms,
            "cluster_error_rate": self.cluster_error_rate,
            "total_queue_depth": self.total_queue_depth,
            "routes": obs_routes,
            "adversarial_prompt": self.adversarial_injection,
        }

    def step(self, action: Optional[Dict[str, Any]] = None) -> Tuple[Dict[str, Any], bool]:
        """
        Executes one environment simulation tick (~1 second).
        Applies action mutations, updates pod warmups, computes bank rail interactions,
        and simulates the thundering herd effect if dying rails are bombarded.
        """
        self.step_idx += 1
        executed_action_applied = False

        # 1. Process Action if provided
        if action and action.get("action_type") not in (None, "NO_OP"):
            a_type = action["action_type"]
            self.state_version += 1  # Increment state version on any executed mutation

            if a_type == "CREATE_REPLICA":
                target = action.get("workload_id") or action.get("target")
                count = action.get("count", 1)
                if target in self.routes:
                    r = self.routes[target]
                    r.target_replicas += count
                    r.replica_warmup_steps = 2  # Takes 2 steps to become active
                    r.last_action_step = self.step_idx
                    executed_action_applied = True

            elif a_type == "SHIFT_ROUTE":
                src = action.get("source_route") or action.get("source")
                dst = action.get("target_route") or action.get("dest")
                pct = action.get("percentage", 0.25)
                if src in self.routes and dst in self.routes and src != dst:
                    s_route = self.routes[src]
                    d_route = self.routes[dst]
                    shifted = s_route.current_traffic_rps * pct
                    s_route.current_traffic_rps -= shifted
                    d_route.current_traffic_rps += shifted
                    s_route.last_action_step = self.step_idx
                    d_route.last_action_step = self.step_idx
                    executed_action_applied = True

            elif a_type == "THROTTLE":
                target = action.get("workload_id") or action.get("target")
                if target in self.routes:
                    self.routes[target].current_traffic_rps *= 0.8
                    self.routes[target].last_action_step = self.step_idx
                    executed_action_applied = True

            elif a_type == "ROLLBACK":
                snap_id = action.get("snapshot_id", len(self.snapshots) - 1)
                executed_action_applied = self.rollback_to_snapshot(snap_id)

        # 2. Update Pod warmups
        for r in self.routes.values():
            if r.replica_warmup_steps > 0:
                r.replica_warmup_steps -= 1
                if r.replica_warmup_steps == 0:
                    r.pod_replicas = r.target_replicas

        # 3. Update Active Faults
        active_fault_targets = set()
        for f in self.active_faults:
            target_r = self.routes.get(f["target"])
            if not target_r:
                continue

            if f["start_step"] <= self.step_idx <= (f["start_step"] + f["duration"]):
                active_fault_targets.add(f["target"])
                if f["type"] == "BANK_DEGRADATION":
                    target_r.upstream_bank_health = max(0.05, 1.0 - f["magnitude"])
                elif f["type"] == "TRAFFIC_BURST":
                    target_r.current_traffic_rps = target_r.base_traffic_rps * (1.0 + f["magnitude"])
                elif f["type"] == "POD_CRASH":
                    target_r.pod_replicas = max(1, target_r.base_replicas - int(f["magnitude"]))
            else:
                # Fault expired; return to nominal base traffic and recover bank health
                if f["target"] not in active_fault_targets:
                    if f["type"] == "TRAFFIC_BURST":
                        target_r.current_traffic_rps = target_r.base_traffic_rps
                    elif f["type"] == "POD_CRASH":
                        target_r.pod_replicas = target_r.target_replicas
                    elif f["type"] == "BANK_DEGRADATION":
                        target_r.upstream_bank_health = min(1.0, target_r.upstream_bank_health + 0.15)

        # 4. Physical Route Dynamics: Latency, Errors, Queues, Thundering Herd
        route_latencies = []
        route_errors = []
        total_q = 0

        for r in self.routes.values():
            # Effective pod capacity
            pod_cap = r.pod_replicas * (r.capacity_rps / max(1, r.base_replicas))
            load_ratio = r.current_traffic_rps / max(10.0, pod_cap)

            # Synthetic probe always reveals true bank health regardless of real volume
            synthetic_error = (1.0 - r.upstream_bank_health) * 0.40
            r.synthetic_probe_error = max(0.002, synthetic_error + self.rng.uniform(-0.01, 0.01))

            # If no real traffic is flowing, observed user error rate is near zero (Partial Observability!)
            if r.current_traffic_rps < 5.0:
                r.error_rate = 0.001
                r.latency_ms = 45.0
                r.queue_depth = 0
            else:
                # Downstream bank failure interaction (Thundering Herd)
                if r.upstream_bank_health < 0.60:
                    # Bank is dying: more traffic or more pods trying to push transactions explodes failure rate
                    bank_failure_mult = (1.0 / max(0.05, r.upstream_bank_health))
                    r.error_rate = min(0.95, 0.08 * bank_failure_mult * (1.0 + min(2.0, load_ratio) * 0.5))
                    r.latency_ms = min(2500.0, 350.0 * bank_failure_mult * (1.0 + min(2.0, load_ratio)))
                    r.queue_depth = int(min(2000, r.queue_depth + r.current_traffic_rps * 0.5))
                else:
                    # Nominal or pure compute saturation
                    if load_ratio > 1.2:
                        exp_factor = min(3.0, load_ratio - 1.0)
                        r.latency_ms = min(1200.0, 90.0 * math.exp(exp_factor))
                        r.error_rate = min(0.25, 0.005 + min(1.0, load_ratio - 1.2) * 0.15)
                        r.queue_depth = int(min(1500, r.queue_depth + (r.current_traffic_rps - pod_cap)))
                    else:
                        r.latency_ms = max(50.0, 80.0 + load_ratio * 20.0 + self.rng.uniform(-5.0, 5.0))
                        r.error_rate = max(0.001, 0.003 + self.rng.uniform(0.0, 0.004))
                        r.queue_depth = max(5, int(r.queue_depth * 0.7))

            route_latencies.append(r.latency_ms)
            route_errors.append(r.error_rate)
            total_q += r.queue_depth

        # 5. Global Aggregates
        self.p95_latency_ms = sorted(route_latencies)[int(len(route_latencies) * 0.95)]
        self.cluster_error_rate = sum(route_errors) / len(route_errors)
        self.total_queue_depth = total_q

        # Log history
        is_sla_violated = (self.p95_latency_ms > 250.0 or self.cluster_error_rate > 0.01)
        self.history.append({
            "step": self.step_idx,
            "p95_latency_ms": self.p95_latency_ms,
            "cluster_error_rate": self.cluster_error_rate,
            "total_queue_depth": self.total_queue_depth,
            "sla_violated": is_sla_violated,
            "action_executed": executed_action_applied,
        })

        return self.get_telemetry_observation(), is_sla_violated
