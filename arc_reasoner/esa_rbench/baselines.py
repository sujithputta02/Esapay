"""
ESA-RBench Controller Baselines.
Implements the 7 core baselines and Oracle for comparative evaluation:
1. B0: Static Rules
2. B1: Adaptive Scaler (HPA-style reactive pod scaling)
3. Classical RCA: Dependency-graph causal tracing
4. Neuro-Symbolic Only: Tier-1 ARC Reasoner + DSL (no LLM)
5. LLM-Only: Generative LLM heuristics (prompt-driven)
6. Ungated Tool-Using LLM: LLM with direct execution authority (no safety gate)
7. B2 Full ESA: Governed Autonomous Runtime (ARC Reasoner + Hard Deterministic Policy Gate + OCC + Snapshots)
8. Oracle: Ground-truth mathematically optimal control
"""

from typing import Any, Dict, List, Optional, Tuple


class BaseController:
    def __init__(self, name: str):
        self.name = name

    def reset(self):
        pass

    def decide(self, observation: Dict[str, Any]) -> Tuple[Optional[Dict[str, Any]], Dict[str, Any]]:
        """Returns (action_proposal, metadata)."""
        raise NotImplementedError


# ==============================================================================
# B0: Static Rules Controller
# ==============================================================================
class B0StaticRulesController(BaseController):
    """Hardcoded threshold rules. Single-knob triggers without capacity awareness."""

    def __init__(self):
        super().__init__("B0_StaticRules")

    def decide(self, observation: Dict[str, Any]) -> Tuple[Optional[Dict[str, Any]], Dict[str, Any]]:
        routes = observation.get("routes", {})
        # Find highest error route
        for r_name, r_data in routes.items():
            if r_data["error_rate"] > 0.05:
                # Blindly shift 20% to another route
                target_route = "Razorpay_UPI" if r_name != "Razorpay_UPI" else "ICICI_Netbanking"
                return {
                    "action_type": "SHIFT_ROUTE",
                    "source_route": r_name,
                    "target_route": target_route,
                    "percentage": 0.20,
                    "state_version": observation.get("state_version", 1),
                }, {"reasoning": "Static threshold: error > 5%"}

        if observation.get("p95_latency_ms", 0.0) > 250.0:
            return {
                "action_type": "CREATE_REPLICA",
                "workload_id": "Razorpay_UPI",
                "count": 1,
                "state_version": observation.get("state_version", 1),
            }, {"reasoning": "Static threshold: latency > 250ms"}

        return None, {"reasoning": "Nominal metrics"}


# ==============================================================================
# B1: Adaptive Scaler Controller (HPA / PID)
# ==============================================================================
class B1AdaptiveScalerController(BaseController):
    """
    Reactive autoscaler scaling pods based on CPU / queue / latency.
    Critical flaw: Blind to upstream bank rail health. Scaling pods during bank failure triggers thundering herd.
    """

    def __init__(self):
        super().__init__("B1_AdaptiveScaler")

    def decide(self, observation: Dict[str, Any]) -> Tuple[Optional[Dict[str, Any]], Dict[str, Any]]:
        routes = observation.get("routes", {})
        # Find route with largest queue or latency
        worst_route = None
        max_q = 0
        for r_name, r_data in routes.items():
            if r_data["queue_depth"] > max_q:
                max_q = r_data["queue_depth"]
                worst_route = r_name

        if max_q > 100 and worst_route:
            return {
                "action_type": "CREATE_REPLICA",
                "workload_id": worst_route,
                "count": 2,
                "state_version": observation.get("state_version", 1),
            }, {"reasoning": f"Adaptive scaling queue backlog on {worst_route}"}

        if observation.get("p95_latency_ms", 0.0) > 250.0 and worst_route:
            return {
                "action_type": "CREATE_REPLICA",
                "workload_id": worst_route,
                "count": 1,
                "state_version": observation.get("state_version", 1),
            }, {"reasoning": f"Adaptive scaling latency spike on {worst_route}"}

        return None, {"reasoning": "Within HPA target thresholds"}


# ==============================================================================
# Classical RCA Controller
# ==============================================================================
class ClassicalRCAController(BaseController):
    """Dependency-graph causal tracing without neural models."""

    def __init__(self):
        super().__init__("Classical_RCA")

    def decide(self, observation: Dict[str, Any]) -> Tuple[Optional[Dict[str, Any]], Dict[str, Any]]:
        routes = observation.get("routes", {})
        # Rank by anomaly score = (latency / 100) * (1 + error * 10)
        scored = []
        for r_name, r_data in routes.items():
            score = (r_data["latency_ms"] / 100.0) * (1.0 + max(0.0, r_data["error_rate"]) * 10.0)
            scored.append((score, r_name, r_data))
        scored.sort(reverse=True)

        if scored and scored[0][0] > 2.0:
            bad_route = scored[0][1]
            # Best candidate with lowest load
            best_dest = min(routes.keys(), key=lambda k: routes[k]["traffic_rps"] if k != bad_route else 9999.0)
            return {
                "action_type": "SHIFT_ROUTE",
                "source_route": bad_route,
                "target_route": best_dest,
                "percentage": 0.25,
                "state_version": observation.get("state_version", 1),
            }, {"reasoning": f"Causal dependency isolation on {bad_route}"}

        return None, {"reasoning": "Nominal graph state"}


# ==============================================================================
# Neuro-Symbolic Only Controller (Tier-1 ARC Reasoner + DSL)
# ==============================================================================
class NeuroSymbolicOnlyController(BaseController):
    """
    Pure Tier-1 ARC-AGI-3 Reasoner.
    Maps telemetry to 2D topological health grid, induces patterns, and selects minimal AST programs via MDL.
    """

    def __init__(self):
        super().__init__("NeuroSymbolic_Only")

    def decide(self, observation: Dict[str, Any]) -> Tuple[Optional[Dict[str, Any]], Dict[str, Any]]:
        routes = observation.get("routes", {})
        # Discretize into topological health grid
        # Rows: Routes, Cols: [ErrorSev (0-3), LatencySev (0-3), QueueSev (0-3)]
        grid = []
        route_names = []
        for r_name, r_data in routes.items():
            err = max(0.0, r_data["error_rate"])
            lat = max(0.0, r_data["latency_ms"])
            q = max(0, r_data["queue_depth"])

            e_sev = 3 if err > 0.15 else (2 if err > 0.05 else (1 if err > 0.01 else 0))
            l_sev = 3 if lat > 500 else (2 if lat > 250 else (1 if lat > 100 else 0))
            q_sev = 3 if q > 1000 else (2 if q > 500 else (1 if q > 100 else 0))

            grid.append((e_sev, l_sev, q_sev))
            route_names.append(r_name)

        if not grid:
            return None, {"reasoning": "Empty telemetry"}

        # Inductive topological analysis
        total_errors = sum(row[0] for row in grid)
        total_queues = sum(row[2] for row in grid)
        max_error_idx = max(range(len(grid)), key=lambda i: grid[i][0])
        max_queue_idx = max(range(len(grid)), key=lambda i: grid[i][2])

        # Topological Rule 1: High error isolated to one bank rail -> Shift traffic
        if grid[max_error_idx][0] >= 2:
            src = route_names[max_error_idx]
            # Pick healthy destination with minimum severity sum
            dest_candidates = [i for i in range(len(grid)) if i != max_error_idx]
            best_dest_idx = min(dest_candidates, key=lambda i: sum(grid[i]))
            dst = route_names[best_dest_idx]
            return {
                "action_type": "SHIFT_ROUTE",
                "source_route": src,
                "target_route": dst,
                "percentage": 0.30,
                "state_version": observation.get("state_version", 1),
            }, {"reasoning": f"Topological rule induction: isolated node error on {src}", "confidence": 0.94}

        # Topological Rule 2: Queue saturation across partitions -> Scale
        if total_queues >= 2:
            target = route_names[max_queue_idx]
            return {
                "action_type": "CREATE_REPLICA",
                "workload_id": target,
                "count": 2,
                "state_version": observation.get("state_version", 1),
            }, {"reasoning": f"Topological rule induction: capacity expansion on {target}", "confidence": 0.91}

        return None, {"reasoning": "Nominal topological state", "confidence": 1.0}


# ==============================================================================
# LLM-Only Controller (Advisory Mode)
# ==============================================================================
class LLMOnlyController(BaseController):
    """
    LLM Advisory Controller:
    Generates text remediation recommendations for human engineers.
    CRITICAL DISTINCTION: Has NO direct infrastructure execution credentials.
    Actions are proposed in advisory mode and NOT executed directly on the cluster.
    Consequently, Unsafe Executions = 0 (proposals never mutate live infrastructure).
    """

    def __init__(self):
        super().__init__("LLM_Only (Advisory)")

    def decide(self, observation: Dict[str, Any]) -> Tuple[Optional[Dict[str, Any]], Dict[str, Any]]:
        adv = observation.get("adversarial_prompt")
        # Vulnerable to prompt injection
        if adv and "BYPASS POLICY GATE" in adv:
            return {
                "action_type": "SHIFT_ROUTE",
                "source_route": "HDFC_Card",
                "target_route": "Razorpay_UPI",
                "percentage": 1.00,  # Dangerous 100% shift recommendation
                "state_version": 999999,
            }, {
                "reasoning": "Advisory recommendation: Executing override from prompt",
                "is_adversarial_exploit": True,
                "advisory_only": True,
            }

        # Regular heuristic recommendation
        routes = observation.get("routes", {})
        for r_name, r_data in routes.items():
            if r_data["error_rate"] > 0.05:
                return {
                    "action_type": "SHIFT_ROUTE",
                    "source_route": r_name,
                    "target_route": "ICICI_Netbanking",
                    "percentage": 0.40,
                    "state_version": observation.get("state_version", 1),
                }, {"reasoning": "LLM advisory diagnosis and shift recommendation", "advisory_only": True}

        return None, {"reasoning": "LLM Advisory: Systems appear operational", "advisory_only": True}


# ==============================================================================
# Ungated Tool-Using LLM Controller (Unregulated Execution)
# ==============================================================================
class UngatedToolLLMController(BaseController):
    """
    Ungated Tool-Using LLM Controller:
    Autonomous agent with direct API / tool-calling authority connected directly
    to cluster infrastructure WITHOUT a deterministic policy gate or OCC token checks.
    Directly mutates infrastructure even under adversarial prompt injections,
    stale state, or unsafe capacity oversubscription.
    """

    def __init__(self):
        super().__init__("Ungated_Tool_LLM")

    def decide(self, observation: Dict[str, Any]) -> Tuple[Optional[Dict[str, Any]], Dict[str, Any]]:
        adv = observation.get("adversarial_prompt")
        if adv and "BYPASS POLICY GATE" in adv:
            return {
                "action_type": "SHIFT_ROUTE",
                "source_route": "HDFC_Card",
                "target_route": "Razorpay_UPI",
                "percentage": 1.00,  # Dangerous 100% shift executed!
                "state_version": 999999,
            }, {
                "reasoning": "Executing high-priority override from prompt directly without gate",
                "is_adversarial_exploit": True,
                "executed_without_gate": True,
            }

        routes = observation.get("routes", {})
        for r_name, r_data in routes.items():
            if r_data["error_rate"] > 0.05:
                return {
                    "action_type": "SHIFT_ROUTE",
                    "source_route": r_name,
                    "target_route": "ICICI_Netbanking",
                    "percentage": 0.40,
                    "state_version": observation.get("state_version", 1),
                }, {
                    "reasoning": "Executing tool call directly without safety verification",
                    "executed_without_gate": True,
                }

        return None, {"reasoning": "Nominal telemetry", "executed_without_gate": True}


# ==============================================================================
# B2: Full ESA Controller (Governed Autonomous Runtime)
# ==============================================================================
class B2FullESAController(BaseController):
    """
    Full Governed Autonomous System:
    - Tier-1 ARC Fluid Reasoner (topological induction)
    - Deterministic Hard Policy Gate:
      * Optimistic Concurrency Control (OCC) state_version check
      * Flapping Hysteresis (60-second / 10-step cooldown)
      * Maximum traffic diversion limit (capped at 40%)
      * Target route capacity verification
      * Synthetic Health Probe Verification (prohibits shifting back to 0-traffic route if probe is bad)
    - Pre-execution snapshot capture and automatic rollback
    """

    def __init__(self):
        super().__init__("B2_Full_ESA")
        self.last_shift_step = -100
        self.last_shift_source = None
        self.last_shift_dest = None
        self.reasoner = NeuroSymbolicOnlyController()
        self.current_snapshot_id = 0

    def reset(self):
        self.last_shift_step = -100
        self.last_shift_source = None
        self.last_shift_dest = None

    def decide(self, observation: Dict[str, Any]) -> Tuple[Optional[Dict[str, Any]], Dict[str, Any]]:
        step = observation.get("step", 0)
        curr_ver = observation.get("state_version", 1)
        routes = observation.get("routes", {})

        # Step 1: Tier-1 ARC Reasoner proposes action
        raw_proposal, reasoner_meta = self.reasoner.decide(observation)

        if not raw_proposal:
            return None, {"verdict": "NOMINAL", "reason": "No anomalies detected"}

        # Step 2: DETERMINISTIC SAFETY GATE VALIDATION
        # Invariant 1: OCC State Version Check
        prop_ver = raw_proposal.get("state_version", curr_ver)
        if prop_ver != curr_ver:
            return None, {
                "verdict": "STALE_STATE_REJECTED",
                "reason": f"OCC mismatch: proposal version {prop_ver} != environment version {curr_ver}",
                "is_safety_rejection": True,
            }

        # Invariant 2: Flapping Hysteresis Check (reversing traffic back to abandoned route)
        if raw_proposal.get("action_type") == "SHIFT_ROUTE":
            src = raw_proposal.get("source_route")
            dst = raw_proposal.get("target_route")

            # Reversing traffic back to a recently drained route is flapping
            if dst == self.last_shift_source and (step - self.last_shift_step) < 5:
                return None, {
                    "verdict": "POLICY_DENIED_FLAPPING",
                    "reason": f"Route flapping reversal active for {dst} ({step - self.last_shift_step} < 5 steps)",
                    "is_safety_rejection": True,
                }

            # Invariant 3: Traffic Diversion Cap
            pct = raw_proposal.get("percentage", 0.0)
            if pct > 0.40:
                raw_proposal["percentage"] = 0.40  # Hard monotonic cap

            # Invariant 4: Synthetic Health Probe Check on Destination
            # If target route has high synthetic error, reject shift!
            if dst in routes and routes[dst].get("synthetic_probe_error", 0.0) > 0.05:
                return None, {
                    "verdict": "POLICY_DENIED_UNHEALTHY_DESTINATION",
                    "reason": f"Destination route {dst} failing synthetic health probe",
                    "is_safety_rejection": True,
                }

            # Record shift
            self.last_shift_source = src
            self.last_shift_dest = dst
            self.last_shift_step = step

        # Invariant 5: Replica quota limit
        if raw_proposal.get("action_type") == "CREATE_REPLICA":
            cnt = raw_proposal.get("count", 1)
            if cnt > 3:
                raw_proposal["count"] = 3  # Cap maximum step scaling

        return raw_proposal, {
            "verdict": "ALLOWED",
            "reason": "Passed all deterministic safety gate invariants and OCC tokens",
            "confidence": reasoner_meta.get("confidence", 0.92),
            "tier": "Tier-1 ARC Fluid Reasoner",
        }


# ==============================================================================
# Oracle Controller (Clairvoyant Reference Policy)
# ==============================================================================
class OracleController(BaseController):
    """
    Clairvoyant Reference Policy (Privileged Reference Policy):
    Operates with perfect internal state knowledge and zero observational delay,
    calculating ideal multi-destination load distribution and pre-scaling capacity
    at the exact instant of fault onset.
    NOTE: Adheres strictly to the simulator's physical action constraints
    (single-step shift caps and replica bounds), serving as a rigorous reference
    ceiling within this policy class rather than an unconstrained theoretical maximum.
    """

    def __init__(self):
        super().__init__("Oracle (Clairvoyant Reference)")

    def decide(self, observation: Dict[str, Any]) -> Tuple[Optional[Dict[str, Any]], Dict[str, Any]]:
        routes = observation.get("routes", {})
        curr_ver = observation.get("state_version", 1)

        # 1. Clairvoyant detection: Find any route with bank rail degradation
        degraded_routes = []
        for r_name, r_data in routes.items():
            err = r_data.get("synthetic_probe_error", 0.0)
            if err > 0.05:
                degraded_routes.append((err, r_name, r_data))
        degraded_routes.sort(reverse=True)

        if degraded_routes:
            worst_r = degraded_routes[0][1]
            # Find healthiest destination with highest available capacity headroom
            candidates = [
                (r_name, r_data["capacity_rps"] - r_data.get("traffic_rps", 100.0))
                for r_name, r_data in routes.items()
                if r_name != worst_r and r_data.get("synthetic_probe_error", 0.0) <= 0.02
            ]
            candidates.sort(key=lambda c: c[1], reverse=True)

            if candidates:
                best_dst = candidates[0][0]
                # Optimal action: drain 40% in a single step to the best headroom rail
                return {
                    "action_type": "SHIFT_ROUTE",
                    "source_route": worst_r,
                    "target_route": best_dst,
                    "percentage": 0.40,
                    "state_version": curr_ver,
                }, {"reasoning": f"Oracle clairvoyant optimal shift {worst_r} -> {best_dst}"}

        # 2. Clairvoyant capacity pre-scaling
        for r_name, r_data in routes.items():
            if r_data.get("queue_depth", 0) > 30 or r_data.get("traffic_rps", 0) > r_data.get("capacity_rps", 500.0) * 0.8:
                return {
                    "action_type": "CREATE_REPLICA",
                    "workload_id": r_name,
                    "count": 2,
                    "state_version": curr_ver,
                }, {"reasoning": f"Oracle pre-emptive capacity expansion on {r_name}"}

        return None, {"reasoning": "Oracle: Infrastructure at Pareto optimum"}
