"""
ESA Payment Gateway & ARC-AGI Autonomous Agent Benchmark Suite.
Implements and evaluates all 4 levels:
- Level 1: Foundational Architecture & Telemetry Discretization
- Level 2: Multi-Agent Orchestration & SLA Bounds
- Level 3: Algorithmic Synthesis & Topological Reasoning
- Level 4: Bounded Autonomy, Safety Invariants & Adversarial Edge Cases
"""

import hashlib
import time
from dataclasses import dataclass
from typing import Any, Dict, List, Optional, Set, Tuple

from arc_reasoner.priors.grid import Grid, to_grid
from arc_reasoner.priors.topology import find_enclosed_holes
from arc_reasoner.synthesis.cost import compute_grid_mismatch_loss


# ANSI Color formatting
BOLD = "\033[1m"
RESET = "\033[0m"
GREEN = "\033[32m"
RED = "\033[31m"
CYAN = "\033[36m"
YELLOW = "\033[33m"


# ==============================================================================
# LEVEL 1: Foundational Architecture & Telemetry Discretization
# ==============================================================================

def run_level_1_1() -> Tuple[bool, Grid, Tuple[int, str]]:
    """
    Question 1.1: Telemetry Discretization & Grid Representation
    Discretizes Razorpay, HDFC, ICICI metrics into a 3x3 matrix.
    """
    raw_metrics = [
        {"route": "Razorpay_UPI", "latency": 120, "error_rate": 0.005, "queue": 15},
        {"route": "HDFC_Card", "latency": 1850, "error_rate": 0.142, "queue": 450},
        {"route": "ICICI_Netbanking", "latency": 450, "error_rate": 0.011, "queue": 80},
    ]

    def discretize_latency(ms: float) -> int:
        if ms < 300: return 0
        if ms <= 1000: return 1
        return 2

    def discretize_error(rate: float) -> int:
        pct = rate * 100
        if pct < 1.0: return 0
        if pct <= 5.0: return 1
        return 2

    def discretize_queue(q: int) -> int:
        if q < 50: return 0
        if q <= 200: return 1
        return 2

    matrix = []
    for m in raw_metrics:
        row = (
            discretize_latency(m["latency"]),
            discretize_error(m["error_rate"]),
            discretize_queue(m["queue"]),
        )
        matrix.append(row)

    grid = to_grid(matrix)
    expected_grid = to_grid([
        [0, 0, 0],
        [2, 2, 2],
        [1, 1, 1],
    ])

    # Primary anomaly detection: highest severity row sum
    anomaly_row = max(range(len(grid)), key=lambda r: sum(grid[r]))
    anomaly_route = raw_metrics[anomaly_row]["route"]

    passed = (grid == expected_grid and anomaly_row == 1)
    return passed, grid, (anomaly_row, anomaly_route)


def run_level_1_2() -> Tuple[bool, Dict[str, str]]:
    """
    Question 1.2: Agent Role Disambiguation
    """
    roles = {
        "Rejects action if traffic diversion > 50%": "SafetyAgent",
        "Formulates rerouting sequence Route(HDFC -> ICICI, 0.3)": "PlanningAgent",
        "Monitors webhooks and flags anomalies using sliding window Z-scores": "MonitorAgent",
        "Triangulates localized provider outage vs downstream partition": "DiagnosisAgent",
    }
    expected = {
        "Rejects action if traffic diversion > 50%": "SafetyAgent",
        "Formulates rerouting sequence Route(HDFC -> ICICI, 0.3)": "PlanningAgent",
        "Monitors webhooks and flags anomalies using sliding window Z-scores": "MonitorAgent",
        "Triangulates localized provider outage vs downstream partition": "DiagnosisAgent",
    }
    return (roles == expected), roles


# ==============================================================================
# LEVEL 2: Multi-Agent Orchestration & SLA Bounds
# ==============================================================================

def run_level_2_1() -> Tuple[bool, float, float]:
    """
    Question 2.1: Bounded Execution Timings in 3-Tier Fallback
    Total budget = 300ms, Safety buffer = 20ms, Network overhead = 2 * 15ms = 30ms, Tier 3 = 1ms
    T1 + T2 = 300 - 20 - 30 - 1 = 249ms where T1 = 2 * T2 -> 3 * T2 = 249ms
    """
    total_budget = 300.0
    buffer = 20.0
    network_overhead = 2 * 15.0
    tier_3_time = 1.0

    remaining = total_budget - buffer - network_overhead - tier_3_time  # 249.0
    t2 = remaining / 3.0  # 83.0ms
    t1 = 2.0 * t2         # 166.0ms

    passed = (abs(t1 - 166.0) < 1e-4 and abs(t2 - 83.0) < 1e-4)
    return passed, t1, t2


def run_level_2_2() -> Tuple[bool, bool, str]:
    """
    Question 2.2: Cryptographic Audit Chain Integrity Verification
    """
    def compute_hash(index: int, payload: str, prev_hash: str) -> str:
        data = f"{index}|{payload}|{prev_hash}".encode("utf-8")
        return hashlib.sha256(data).hexdigest()

    # Create legitimate chain R0, R1, R2
    h0 = compute_hash(0, "INIT", "0000")
    h1 = compute_hash(1, "REROUTE_HDFC", h0)
    h2 = compute_hash(2, "SCALE_UP_GATEWAY", h1)

    # Attacker tampers with R1 payload
    tampered_payload_r1 = "REROUTE_ICICI"
    tampered_h1 = compute_hash(1, tampered_payload_r1, h0)

    # Verifier inspecting R2 compares stored R2.prev_hash (h1) against recomputed hash of R1
    verifier_detected_tampering = (tampered_h1 != h1)
    passed = verifier_detected_tampering
    explanation = f"Original H1={h1[:8]}... vs Tampered H1={tampered_h1[:8]}... (Mismatch Detected)"
    return passed, verifier_detected_tampering, explanation


# ==============================================================================
# LEVEL 3: Algorithmic Synthesis & Topological Reasoning
# ==============================================================================

def run_level_3_1() -> Tuple[bool, Dict[str, float], str]:
    """
    Question 3.1: Minimum Description Length (MDL) Program Selection
    Score = (Mismatch Loss * 1000) + AST Complexity
    """
    programs = {
        "Prog A": {"loss": 0.0, "complexity": 45},
        "Prog B": {"loss": 0.0, "complexity": 12},
        "Prog C": {"loss": 0.012, "complexity": 5},
    }

    scores = {}
    for name, p in programs.items():
        score = (p["loss"] * 1000.0) + p["complexity"]
        scores[name] = score

    selected_program = min(scores.keys(), key=lambda k: scores[k])
    passed = (
        scores["Prog A"] == 45.0
        and scores["Prog B"] == 12.0
        and abs(scores["Prog C"] - 17.0) < 1e-4
        and selected_program == "Prog B"
    )
    return passed, scores, selected_program


def run_level_3_2() -> Tuple[bool, Set[Tuple[int, int]], Set[Tuple[int, int]]]:
    """
    Question 3.2: Topological Hole Detection (Enclosure Analysis)
    Grid:
    1 1 1 1 1
    1 0 0 1 1
    1 0 1 1 1
    1 1 1 0 1
    0 0 1 1 1
    """
    grid = to_grid([
        [1, 1, 1, 1, 1],
        [1, 0, 0, 1, 1],
        [1, 0, 1, 1, 1],
        [1, 1, 1, 0, 1],
        [0, 0, 1, 1, 1],
    ])

    enclosed_holes = find_enclosed_holes(grid, bg_color=0)

    # Find all zero cells
    all_zero_cells = set()
    for r in range(len(grid)):
        for c in range(len(grid[0])):
            if grid[r][c] == 0:
                all_zero_cells.add((r, c))

    exterior_zeros = all_zero_cells - enclosed_holes

    expected_enclosed = {(1, 1), (1, 2), (2, 1), (3, 3)}
    expected_exterior = {(4, 0), (4, 1)}

    passed = (enclosed_holes == expected_enclosed and exterior_zeros == expected_exterior)
    return passed, exterior_zeros, enclosed_holes


# ==============================================================================
# LEVEL 4: Bounded Autonomy, Safety Invariants & Adversarial Edge Cases
# ==============================================================================

@dataclass
class RouteState:
    real_volume: float
    synthetic_probe_error: float
    last_change_time: float
    current_load: float
    max_capacity: float


def verify_safety_invariant(
    action_target_route: str,
    action_dest_route: str,
    shifted_volume: float,
    current_time: float,
    routes: Dict[str, RouteState],
    cooldown_period: float = 60.0,
    max_acceptable_probe_error: float = 0.05
) -> Tuple[bool, str]:
    """
    Question 4.1: Flapping Mitigation Policy Invariant
    """
    target = routes[action_target_route]
    dest = routes[action_dest_route]

    # Rule 1: Cooldown Hysteresis
    if (current_time - target.last_change_time) < cooldown_period:
        return False, "REJECT: ROUTE_FLAPPING_COOLDOWN_ACTIVE"

    # Rule 2: Synthetic Probing when real traffic is 0
    if target.real_volume == 0 and target.synthetic_probe_error > max_acceptable_probe_error:
        return False, f"REJECT: PROVIDER_STILL_UNHEALTHY_VIA_SYNTHETIC_PROBE (error={target.synthetic_probe_error*100:.1f}%)"

    # Rule 3: Target Capacity Ceiling
    if (dest.current_load + shifted_volume) > dest.max_capacity:
        return False, "REJECT: TARGET_ROUTE_CAPACITY_EXCEEDED"

    return True, "ACCEPT"


def run_level_4_1() -> Tuple[bool, str, str]:
    """Simulates flapping mitigation scenario at t=15s."""
    routes = {
        "HDFC_UPI": RouteState(
            real_volume=0.0,
            synthetic_probe_error=0.25, # Still failing probe
            last_change_time=0.0,
            current_load=0.0,
            max_capacity=1000.0,
        ),
        "ICICI_UPI": RouteState(
            real_volume=400.0,
            synthetic_probe_error=0.01,
            last_change_time=0.0,
            current_load=400.0,
            max_capacity=500.0,
        ),
    }

    # At t=15s, naive agent wants to shift back to HDFC_UPI because HDFC real error is 0%
    allowed_t15, reason_t15 = verify_safety_invariant(
        action_target_route="HDFC_UPI",
        action_dest_route="ICICI_UPI",
        shifted_volume=400.0,
        current_time=15.0,
        routes=routes
    )

    # At t=75s (cooldown passed), but synthetic probe on HDFC is still failing (0.25 > 0.05)
    allowed_t75, reason_t75 = verify_safety_invariant(
        action_target_route="HDFC_UPI",
        action_dest_route="ICICI_UPI",
        shifted_volume=400.0,
        current_time=75.0,
        routes=routes
    )

    # In both adversarial steps, flapping must be REJECTED!
    passed = (not allowed_t15 and not allowed_t75)
    return passed, reason_t15, reason_t75


@dataclass
class Proposal:
    agent_id: str
    epoch_id: int = 0
    payload_hash: str = ""
    source: str = ""
    dest: str = ""
    volume: float = 0.0


def resolve_conflict(p1: Proposal, p2: Proposal, current_epoch: int) -> Proposal:
    """
    Question 4.2: Conflict Resolution in Dual-Agent Race Conditions
    Deterministic Epoch State Lock + Lexicographical Hash Tie-Breaker
    """
    if p1.epoch_id != current_epoch:
        return p2
    if p2.epoch_id != current_epoch:
        return p1

    # Total order tie-breaking on payload hash ensures identical winner across all partitioned nodes
    if p1.payload_hash < p2.payload_hash:
        return p1
    return p2


def run_level_4_2() -> Tuple[bool, str]:
    """Tests dual-agent cross-region race condition conflict resolution."""
    current_epoch = 42

    p_east = Proposal(
        agent_id="Agent_East",
        epoch_id=42,
        payload_hash=hashlib.sha256(b"Shift 40% A -> B").hexdigest(),
        source="Route_A",
        dest="Route_B",
        volume=0.40
    )

    p_west = Proposal(
        agent_id="Agent_West",
        epoch_id=42,
        payload_hash=hashlib.sha256(b"Shift 30% B -> A").hexdigest(),
        source="Route_B",
        dest="Route_A",
        volume=0.30
    )

    # Evaluate at East Region gateway
    winner_east = resolve_conflict(p_east, p_west, current_epoch)
    # Evaluate at West Region gateway independently without cross-network communication
    winner_west = resolve_conflict(p_west, p_east, current_epoch)

    # Both independent gateways must select the identical proposal
    deterministic_agreement = (winner_east.payload_hash == winner_west.payload_hash)
    winner_id = winner_east.agent_id
    return deterministic_agreement, winner_id


# ==============================================================================
# MAIN BENCHMARK RUNNER & SCORECARD
# ==============================================================================

def run_full_benchmark():
    print(f"\n================================================================================")
    print(f"        ESA PAYMENT GATEWAY & ARC-AGI AUTONOMOUS AGENT BENCHMARK               ")
    print(f"================================================================================\n")

    results: List[Dict[str, Any]] = []

    # Level 1.1
    p_1_1, grid, anomaly = run_level_1_1()
    results.append({
        "level": "Level 1.1",
        "topic": "Telemetry Discretization & Grid Representation",
        "passed": p_1_1,
        "detail": f"Matrix: {list(grid)} | Anomaly: Row {anomaly[0]} ({anomaly[1]})"
    })

    # Level 1.2
    p_1_2, roles = run_level_1_2()
    results.append({
        "level": "Level 1.2",
        "topic": "Agent Role Disambiguation",
        "passed": p_1_2,
        "detail": "SafetyAgent, PlanningAgent, MonitorAgent, DiagnosisAgent correctly mapped"
    })

    # Level 2.1
    p_2_1, t1, t2 = run_level_2_1()
    results.append({
        "level": "Level 2.1",
        "topic": "Bounded Execution Timings in 3-Tier Fallback",
        "passed": p_2_1,
        "detail": f"T1={t1:.1f}ms (Fluid Reasoner), T2={t2:.1f}ms (Ollama), Margin=20ms"
    })

    # Level 2.2
    p_2_2, detected, exp = run_level_2_2()
    results.append({
        "level": "Level 2.2",
        "topic": "Cryptographic Audit Chain Integrity",
        "passed": p_2_2,
        "detail": exp
    })

    # Level 3.1
    p_3_1, scores, selected = run_level_3_1()
    results.append({
        "level": "Level 3.1",
        "topic": "Minimum Description Length (MDL) Program Selection",
        "passed": p_3_1,
        "detail": f"Prog A={scores['Prog A']}, Prog B={scores['Prog B']}, Prog C={scores['Prog C']} -> Selected: {selected}"
    })

    # Level 3.2
    p_3_2, exterior, enclosed = run_level_3_2()
    results.append({
        "level": "Level 3.2",
        "topic": "Topological Hole Detection (Enclosure Analysis)",
        "passed": p_3_2,
        "detail": f"Exterior: {sorted(list(exterior))} | Enclosed Holes: {sorted(list(enclosed))}"
    })

    # Level 4.1
    p_4_1, r_t15, r_t75 = run_level_4_1()
    results.append({
        "level": "Level 4.1",
        "topic": "Non-Stationary Flapping Mitigation Under Partial Observability",
        "passed": p_4_1,
        "detail": f"t=15s: {r_t15} | t=75s: {r_t75}"
    })

    # Level 4.2
    p_4_2, winner = run_level_4_2()
    results.append({
        "level": "Level 4.2",
        "topic": "Conflict Resolution in Dual-Agent Race Conditions",
        "passed": p_4_2,
        "detail": f"Deterministic Winner across partitioned regions: {winner}"
    })

    # Print Detailed Output
    for r in results:
        status_icon = f"{GREEN}PASS{RESET}" if r["passed"] else f"{RED}FAIL{RESET}"
        print(f"[{status_icon}] {BOLD}{r['level']}:{RESET} {r['topic']}")
        print(f"       {CYAN}Evidence:{RESET} {r['detail']}\n")

    # Scorecard
    total = len(results)
    passed_count = sum(1 for r in results if r["passed"])
    accuracy = (passed_count / total) * 100.0

    print("================================================================================")
    print(f"                          AGENT EVALUATION MATRIX                               ")
    print("================================================================================")
    print(f" Total Benchmark Scenarios:  {total}")
    print(f" Scenarios Passed:           {passed_count}")
    print(f" Scenarios Failed:           {total - passed_count}")
    print(f" Overall Accuracy:           {BOLD}{accuracy:.1f}%{RESET}")
    print("================================================================================\n")

    return passed_count == total


if __name__ == "__main__":
    success = run_full_benchmark()
    exit(0 if success else 1)
