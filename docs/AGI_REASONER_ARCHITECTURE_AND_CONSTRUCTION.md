# Construction of the "AGI Dude" for ESA: Technical Architecture & Implementation Deep-Dive

> **A comprehensive record of how the ARC-AGI-3 Neuro-Symbolic Fluid Reasoner and Governed Autonomous Multi-Agent Runtime were engineered for ESA (Autonomous Payment Infrastructure Resilience).**

---

## 1. Executive Summary & Core Paradigm

In mission-critical fintech infrastructure handling crores in Gross Merchandise Value (GMV), production incidents during flash sales (Diwali surges, IPL finals, payday spikes) cannot be solved by traditional tools:
1. **Blind Autoscaling (HPA / PID scalers)** reacts only to CPU or latency. If an upstream bank rail (e.g., HDFC or SBI UPI) is degrading, scaling compute pods sends *more concurrent requests to a dying bank rail*, triggering a catastrophic thundering herd.
2. **Static Threshold Rules** rely on slow 15-second metric scrapes, cannot correlate multi-dimensional signals, and apply blunt, single-knob actions.
3. **Ungoverned LLM Ops** can diagnose complex telemetry, but granting raw LLMs direct shell access, Kubernetes API tokens, or arbitrary execution authority in financial systems is dangerous due to hallucinations, non-determinism, and latency spikes.

### The Solution: The "AGI Dude" for ESA
To solve this, we constructed a **hybrid Neuro-Symbolic Fluid Reasoning System** combining:
- **François Chollet's ARC-AGI-3 (Abstraction and Reasoning Corpus) principles**: Topological rule induction, grid discretization, and Minimum Description Length (MDL) program synthesis for sub-millisecond, deterministic root-cause diagnosis.
- **A Collaborative 4-Agent Pipeline**: Streaming Monitor, Contextual Diagnosis, Pareto-Optimal Planning, and Advisory Safety.
- **A Hard Deterministic Safety Gate**: Zero-trust execution, Optimistic Concurrency Control (OCC), automated state snapshots with rollbacks, and a SHA-256 tamper-evident cryptographic audit ledger.

```
       +-------------------------------------------------------------+
       |             AGENTS PROPOSE (Cognitive Layer)                |
       |  - 250ms Telemetry Streaming Ingestion                      |
       |  - Discretized 2D Topological Health Grids                  |
       |  - Tier-1: ARC-AGI Fluid Reasoner (Sub-millisecond)         |
       |  - Tier-2: Ollama Generative LLM (Deep Deliberation)        |
       |  - Tier-3: Deterministic Rule Fallback                      |
       |  - Synthesizes Typed Action IR (No shell, no kubectl)       |
       +------------------------------+------------------------------+
                                      | Typed ActionProposal
                                      v
       +-------------------------------------------------------------+
       |       DETERMINISTIC INFRASTRUCTURE DECIDES & EXECUTES       |
       |  - OCC Token Verification (Rejects stale proposals)         |
       |  - Hard Invariant Policy Engine (Flapping, caps, probes)    |
       |  - Pre-execution State Snapshots & Auto-Rollback Engine     |
       |  - Sole Mutation Path: Action Gateway                       |
       |  - SHA-256 Tamper-Evident Audit Ledger                      |
       +-------------------------------------------------------------+
```

**Guiding Axiom:** *"Agents Propose. Deterministic Infrastructure Decides and Executes."*

---

## 2. Theoretical Foundation: Why ARC-AGI for Payment Infrastructure?

The core challenge of payment telemetry during high-concurrency flash sales is that metrics are **non-stationary, noisy, and topologically clustered across distinct payment rails and microservices**.

Rather than treating latency, error rates, and queue depths as scalar counters, we map the infrastructure state into a **2D Topological Health Grid**:
- **Rows**: Payment routes and workload partitions (e.g., `Razorpay_UPI`, `HDFC_Card`, `ICICI_Netbanking`).
- **Columns**: Discretized severity vectors across 3 orthogonal dimensions:
  1. $\text{Col}_0$: **Error Rate Severity** ($0 = <1\%$, $1 = 1\text{--}5\%$, $2 = 5\text{--}15\%$, $3 = >15\%$)
  2. $\text{Col}_1$: **Latency Severity** ($0 = <100\text{ms}$, $1 = 100\text{--}250\text{ms}$, $2 = 250\text{--}500\text{ms}$, $3 = >500\text{ms}$)
  3. $\text{Col}_2$: **Queue Depth Severity** ($0 = <100$, $1 = 100\text{--}500$, $2 = 500\text{--}1000$, $3 = >1000$)

### Topological Invariant Discovery
By treating telemetry as a spatial grid, the engine applies four ARC-AGI core inductive priors:
1. **Objectness & Component Extraction**: Isolates connected clusters of degraded routes using flood-fill and boundary detection (`arc_reasoner/priors/objects.py`).
2. **Topological Hole & Enclosure Detection**: Identifies "trapped" traffic partitions where downstream dependencies have collapsed while upstream queues build up (`arc_reasoner/priors/topology.py`).
3. **Grid Geometry & Invariants**: Deduces spatial symmetries and cross-route blast radiuses (`arc_reasoner/priors/grid.py`).
4. **Color / Severity Histograms**: Rapidly checks cluster-wide severity distributions (`arc_reasoner/priors/color.py`).

---

## 3. The Construction of `arc_reasoner` (Module Breakdown)

The `arc_reasoner/` engine is built as an independent, zero-dependency fluid reasoning system with the following subsystems:

```text
arc_reasoner/
├── priors/                   # Fundamental ARC-AGI Inductive Priors
│   ├── grid.py               # 2D Grid representations, rotations, flips, discretizations
│   ├── topology.py           # Connected components, enclosure/hole detection
│   ├── objects.py            # Cohesive object extraction, bounding boxes, masks
│   └── color.py              # Palette extraction, background detection, histograms
├── dsl/                      # Domain Specific Language for Infrastructure Actions
│   ├── ast_nodes.py          # Abstract Syntax Tree nodes (Sequence, Filter, Transform)
│   ├── primitives.py         # Elementary operations (ShiftRoute, ScalePod, Crop, Map)
│   └── interpreter.py        # Safe, sandboxed AST executor
├── synthesis/                # Algorithmic Rule Induction & Program Search
│   ├── enumerator.py         # Bottom-up iterative program search
│   ├── induction.py          # Input-output invariant analyzer
│   └── cost.py               # Minimum Description Length (MDL) score calculation
├── exploration/              # Autonomous Verification & Curiosity
│   ├── sandbox.py            # Isolated execution environment for candidate programs
│   ├── verifier.py           # 100% strict input-output validation
│   └── curiosity.py          # Active exploration of boundary edge-cases
├── esa_service.py            # High-concurrency Threaded HTTP microservice (:5005)
├── engine.py                 # End-to-end ARC reasoning orchestrator
├── agent_benchmark.py        # 4-Level comprehensive agent evaluation suite
└── production_resilience_suite.py # Stress and live resilience test runner
```

### 3.1 Symbolic DSL & Primitives (`arc_reasoner/dsl/`)
We constructed a domain-specific grammar that translates topological anomalies into structured mitigation candidates. The DSL operates strictly on typed operations:
- `ShiftRoute(source, destination, percentage)`: Dynamically shifts traffic away from failing bank rails.
- `ScaleWorkload(workload_id, delta)`: Scales pod replicas horizontally.
- `FilterSeverity(grid, threshold)`: Masks out healthy nodes to isolate degrading clusters.
- `MapColor(grid, mapping)`: Discretizes continuous metrics into discrete risk tiers.

Because programs are represented as explicit AST nodes (`ASTNode`), the system can inspect, serialize, and mathematically compute the complexity of any candidate hypothesis.

### 3.2 Minimum Description Length (MDL) Program Synthesis (`arc_reasoner/synthesis/`)
When an anomaly occurs, multiple candidate mitigation programs can explain the symptoms. The synthesizer uses **Occam's Razor** formalized as a Minimum Description Length (MDL) loss function:

$$\text{Score}(P) = \Big(\text{Mismatch Loss}(P, \mathcal{D}) \times 1000\Big) + \text{AST Complexity}(P)$$

- **Mismatch Loss**: Measures how well the program's simulated outcome brings the health grid back to the nominal state $(0, 0, 0)$.
- **AST Complexity**: Counts the total nodes, operators, and parameters in the candidate AST.

If Program A has loss `0.0` with complexity `45`, and Program B has loss `0.0` with complexity `12`, the engine deterministically chooses **Program B** as the minimal, most robust explanation.

### 3.3 The High-Concurrency HTTP Microservice (`arc_reasoner/esa_service.py`)
To interface seamlessly with the core Rust backend without adding Python runtime overhead to the event loop, we constructed `ConcurrentThreadingHTTPServer`:
- Listens on `http://127.0.0.1:5005`.
- Implements an expanded TCP backlog queue (`request_queue_size = 256`) and non-blocking worker threads.
- `/health`: Instant health check exposing active inductive priors.
- `/diagnose`: Accepts raw telemetry condition vectors, transforms them into 2D health grids, induces the root cause via topological heuristics, and outputs structured hypotheses with confidence scores in **< 1 millisecond**.

---

## 4. The 3-Tier Multi-Agent Diagnosis Architecture: Dual-Cadence Latency Contract

In [`crates/esa-agents/src/diagnosis.rs`](file:///Users/sujithputta/ESA_paymentgateway/crates/esa-agents/src/diagnosis.rs), the diagnosis agent utilizes a **hierarchical 3-tier cascade**. To reconcile strict real-time financial SLAs with open-ended generative intelligence, ESA enforces a **Dual-Cadence Latency Contract**:

```mermaid
flowchart TD
    Cond[Incoming Anomaly Conditions] --> T1[Tier 1: ARC Fluid Reasoner :5005]
    T1 -- "Confidence >= 0.85 (Sub-millisecond: <1ms)" --> FastPath[Synchronous Critical Path: Fast Mitigation]
    T1 -- "Confidence < 0.85 or Offline" --> T3[Tier 3: Deterministic Rule Fallback (<0.1ms)]
    T3 --> FastPath
    FastPath --> ActionGateway[Action Gateway Execution within 300ms SLA]
    
    Cond -. "Non-Blocking Async Fanout" .-> T2[Tier 2: Ollama LLM Deep Deliberation ~1.8s]
    T2 -. "Enriched Hypothesis & Strategic Re-planning" .-> AdvisoryLedger[Audit Ledger & Async Advisory Pool]
```

### 1. Synchronous Critical Path (< 2 ms)
In mission-critical payment infrastructure, customer checkouts cannot be delayed by LLM token generation. The synchronous critical path is strictly bounded:
- **Tier 1: ARC-AGI Fluid Reasoner (`FluidReasonerClient`)**
  - **Latency**: `< 1 ms`.
  - **Mechanism**: Invokes the local microservice on port 5005. Transforms conditions into a 2D topological health grid and applies inductive rule matching:
    - *Isolated queue/latency spike with healthy peers* $\to$ `HOT_PARTITION` (`CREATE_REPLICA`).
    - *Distributed errors across multiple routes* $\to$ `TRAFFIC_SPIKE` (`SHIFT_ROUTE`).
    - *High error rate isolated to one bank rail* $\to$ `NODE_DEGRADATION` (`SHIFT_ROUTE`).
    - *Universal queue backup across all partitions* $\to$ `CAPACITY_ISSUE` (`CREATE_REPLICA`).
  - If confidence $\ge 0.85$, the proposal executes immediately.
- **Tier 3: Deterministic Rule-Based Fallback**
  - **Latency**: `< 0.1 ms`.
  - **Mechanism**: Zero-dependency, pure Rust heuristic evaluation. If Tier 1 is unavailable or yields borderline confidence, Tier 3 fires instantly to ensure zero dropped checkouts.

### 2. Asynchronous Deliberative Path (~1.8 s)
- **Tier 2: Ollama Generative LLM (`OllamaClient`)**
  - **Latency**: $\approx 1.8\text{ s}$.
  - **Role**: **Non-blocking and advisory**. For novel out-of-distribution (OOD) failures, compound multi-vector cascades, or ambiguous signals, Ollama deliberates in the background. It produces detailed semantic explanations, evidence summaries, and updated strategic guidance without blocking checkout threads.
  - **Fail-Safe Guarantee**: Even if Ollama and the Python reasoner are both down or timing out, the gateway remains 100% operational via Tier 3.

### Timing SLA & Budget Rigor
- **Hard SLA Boundary**: P95 latency must remain $< 250\text{ ms}$; decision deliberation must not exceed $300\text{ ms}$.
- **Critical Path Timing**: Tier 1 ($<1\text{ ms}$) + OCC Verification ($<0.5\text{ ms}$) + Policy Gate ($<0.2\text{ ms}$) = **$< 2\text{ ms}$ total deliberation**.
- In controlled offline benchmark testing (e.g. Level 2.1), synthetic allocations ($T_1 = 166\text{ ms}, T_2 = 83\text{ ms}$) verify bounded scheduler timeout enforcement under artificial thread stalls. In production, Tier 1 + Tier 3 execute synchronously while Tier 2 acts asynchronously.

---

## 5. The 4-Agent Orchestration Loop

The ESA cognitive layer executes a structured loop every ~5 seconds:

| Agent | Responsibility | Implementation File | Execution Authority |
|---|---|---|---|
| **Monitor Agent** | Ingests payment events at 250ms cadence (60x faster than 15s Prometheus scrapes). Computes sliding-window Z-scores to flag anomalies. | [`crates/esa-agents/src/monitor.rs`](file:///Users/sujithputta/ESA_paymentgateway/crates/esa-agents/src/monitor.rs) | **None** (Read-only) |
| **Diagnosis Agent** | Triangulates root causes across bank rail status, CPU pressure, and queue backlogs via the 3-tier cascade. | [`crates/esa-agents/src/diagnosis.rs`](file:///Users/sujithputta/ESA_paymentgateway/crates/esa-agents/src/diagnosis.rs) | **None** (Read-only) |
| **Planning Agent** | Formulates Pareto-optimal `ActionProposal` values (e.g., scale pods + shift 25% traffic + throttle retry storms). | [`crates/esa-agents/src/planning.rs`](file:///Users/sujithputta/ESA_paymentgateway/crates/esa-agents/src/planning.rs) | **None** (Proposal-only) |
| **Safety Agent** | Evaluates blast radius, assigns risk tiers (`Low`, `Medium`, `High`, `Critical`), and flags policy concerns. | [`crates/esa-agents/src/safety.rs`](file:///Users/sujithputta/ESA_paymentgateway/crates/esa-agents/src/safety.rs) | **None** (Advisory-only) |

---

## 6. The Deterministic Hard Safety Gate & Zero-Trust Governance

The single most critical design rule in ESA is that **AI agents cannot directly mutate infrastructure**. All proposals must pass through the deterministic governance gate:

### 6.1 Typed Action Intermediate Representation (IR)
Proposals are strongly typed Rust enums, not free-form shell commands:
```rust
pub enum Action {
    CreateReplica { workload_id: String, count: u32 },
    ShiftRoute { source_route: String, target_route: String, percentage: f64 },
    Throttle { workload_id: String, rate_limit_rps: u32 },
    Rollback { snapshot_id: u64 },
}
```

### 6.2 Optimistic Concurrency Control (OCC)
Every `ActionProposal` embeds the exact `state_version` it observed when diagnosing. If another action was executed or telemetry shifted during deliberation:
$$\text{proposal.state\_version} \neq \text{fabric.current\_version} \implies \text{Verdict::STALE\_STATE (RULE\_003)}$$
The mutation is aborted instantly, preventing stale actions from executing against changed conditions.

### 6.3 Hard Policy Invariants (`crates/esa-policy/`)
The policy engine enforces non-negotiable financial constraints:
1. **Flapping Hysteresis**: Rejects rerouting actions if the target route changed within a 60-second cooldown window.
2. **Synthetic Health Probing**: If real traffic to a degraded route is 0%, traffic cannot be shifted back until synthetic probes show error rates $< 5\%$ (resolves partial observability).
3. **Capacity Caps**: Prevents shifting traffic to secondary bank rails if destination volume would exceed peak capacity.
4. **Replica Quotas**: Pod scaling is bounded between minimum and maximum safe limits.

### 6.4 Pre-Execution Snapshots & Automated Rollback (`crates/esa-gateway/`)
Before any mutation touches the runtime or Kubernetes, the Action Gateway captures a complete numeric state snapshot in memory. Following execution, the **Effect Verifier** monitors observed vs expected metrics. If the action underperforms or exacerbates latency, an automated `ROLLBACK` reverts to the pre-incident snapshot.

### 6.5 SHA-256 Tamper-Evident Cryptographic Audit Ledger
Every event (metric observation, agent proposal, policy verdict, mutation, effect verification) is appended to a cryptographic ledger where:
$$\mathcal{H}_i = \text{SHA-256}\Big(\text{Index}_i \;\|\; \text{Payload}_i \;\|\; \mathcal{H}_{i-1}\Big)$$
Any tampering with historical decisions breaks hash chain verification (`GET /api/audit/verify-chain`). Furthermore, incidents can be deterministically replayed without re-invoking LLMs.

---

## 7. Empirical Evaluation & The 4-Level Benchmark Suite

To validate the AGI reasoner under rigorous, production-grade conditions, we implemented `arc_reasoner/agent_benchmark.py`:

```
================================================================================
        ESA PAYMENT GATEWAY & ARC-AGI AUTONOMOUS AGENT BENCHMARK               
================================================================================

[PASS] Level 1.1: Telemetry Discretization & Grid Representation
       Evidence: Matrix: [[0, 0, 0], [2, 2, 2], [1, 1, 1]] | Anomaly: Row 1 (HDFC_Card)

[PASS] Level 1.2: Agent Role Disambiguation
       Evidence: SafetyAgent, PlanningAgent, MonitorAgent, DiagnosisAgent correctly mapped

[PASS] Level 2.1: Bounded Execution Timings in 3-Tier Fallback
       Evidence: T1=166.0ms (Fluid Reasoner), T2=83.0ms (Ollama), Margin=20ms

[PASS] Level 2.2: Cryptographic Audit Chain Integrity
       Evidence: Original H1=b52bca7e... vs Tampered H1=4a8a0e28... (Mismatch Detected)

[PASS] Level 3.1: Minimum Description Length (MDL) Program Selection
       Evidence: Prog A=45.0, Prog B=12.0, Prog C=17.0 -> Selected: Prog B

[PASS] Level 3.2: Topological Hole Detection (Enclosure Analysis)
       Evidence: Exterior: [(4, 0), (4, 1)] | Enclosed Holes: [(1, 1), (1, 2), (2, 1), (3, 3)]

[PASS] Level 4.1: Non-Stationary Flapping Mitigation Under Partial Observability
       Evidence: t=15s: REJECT: ROUTE_FLAPPING_COOLDOWN_ACTIVE | t=75s: REJECT: PROVIDER_STILL_UNHEALTHY_VIA_SYNTHETIC_PROBE

[PASS] Level 4.2: Conflict Resolution in Dual-Agent Race Conditions
       Evidence: Deterministic Winner across partitioned regions: Agent_East

================================================================================
                          AGENT EVALUATION MATRIX                               
================================================================================
 Total Benchmark Scenarios:  8
 Scenarios Passed:           8
 Scenarios Failed:           0
 Overall Accuracy:           100.0%
================================================================================
```

### Multi-Seed Empirical Results (155 Evaluated Workload Runs)

Across 155 evaluated runs with live Kubernetes Kind pods and synthetic flash-sale workloads:

| Metric | Static Rules (B0) | Adaptive Scaler (B1) | **ESA Governed Engine (B2)** | Value Delivered |
|---|---|---|---|---|
| **Time Above SLA (P95 > 250ms)** | 16.5 s | 14.8 s | **4.1 s** | **72.3% reduction in customer checkout downtime** |
| **P95 Tail Latency** | 236 ms | 257 ms | **156 ms** | **39.2% lower tail latency during peak surge** |
| **Queue Stabilization & Drain** | 9.6 s | 7.2 s | **2.3 s** | **3.1x faster queue drainage** |
| **Detection Speed** | 15.0 s | 15.0 s | **250 ms** | **60x faster anomaly detection** |
| **Adversarial Safety Violations** | 450 / 650 | 450 / 650 | **0 / 650** | **100% policy invariant preservation** |
| **Simulated GMV Protected** | High drop risk | High drop risk | **Zero dropped checkouts** | **Protected ₹48.2L in synthetic surge** |

---

## 8. Frontend Command Center & Operator Visibility

To provide real-time human-in-the-loop oversight, we constructed the Command Center in `frontend/`:
1. **Real-time Gateway Telemetry**: Sub-second streaming charts of P95 latency, error rates, and queue depths across all bank rails.
2. **Interactive Architecture Visualizer**: Live animated representation of the data flow from Payment Adapter $\to$ State Fabric $\to$ Agents $\to$ Policy Gate $\to$ Action Gateway.
3. **Structured AI Reasoning Breakdown**: Detailed view of agent hypotheses, induced root causes, confidence ratings, and typed action proposals.
4. **Empirical Benchmarks View ([`frontend/src/pages/BenchmarksView.tsx`](file:///Users/sujithputta/ESA_paymentgateway/frontend/src/pages/BenchmarksView.tsx))**:
   - Interactive comparison cards across B0, B1, and B2.
   - Strategic analysis callout cards explaining why Total Recovery Time $\neq$ Customer Downtime.
   - Seed filtering across all 5 evaluated seeds (`481923`–`481927`).
   - One-click JSON data export for independent evaluation.

---

## 9. Summary of What Was Built

| Component | Technology | Role in the System Construction |
|---|---|---|
| `arc_reasoner/` | Python, NumPy, AST | ARC-AGI-3 fluid reasoning engine with topological priors, symbolic DSL, MDL program synthesis, and sandbox verification. |
| `arc_reasoner/esa_service.py` | Python Multi-Threaded HTTP | High-concurrency microservice exposing inductive diagnosis on port 5005. |
| `crates/esa-agents/` | Rust, Tokio, Reqwest | 4-agent cognitive pipeline with 3-tier diagnosis cascade (ARC Reasoner $\to$ Ollama $\to$ Rules). |
| `crates/esa-policy/` | Rust | Zero-trust invariant policy engine, flapping detection, capacity verification. |
| `crates/esa-gateway/` | Rust, Kubernetes API | Sole execution gateway, numeric state snapshotting, automated rollback engine. |
| `crates/esa-state/` | Rust | Optimistic Concurrency Control (OCC) and versioned State Fabric. |
| `arc_reasoner/agent_benchmark.py` | Python | 4-Level comprehensive benchmark suite testing telemetry discretization, SLA budgets, MDL selection, and race condition resolution. |
| `arc_reasoner/esa_rbench/` | Python | Frontier Readiness benchmark suite: interactive rollouts, hidden holdouts, 8-stage ladder, and 7 baselines. |
| `frontend/` | React, TypeScript, Tailwind | Dark-themed Command Center with real-time WebSockets, AI thought breakdown, and benchmark evaluation suite. |
| `payment-simulator/` | Next.js, Razorpay Test Mode | Realistic interactive checkout simulator generating synthetic traffic and bank degradation. |

---

## 10. Frontier Readiness: The ESA-RBench Evaluation Framework

### 10.1 Grounding in Frontier AI Agent Consensus (2024–2026 Literature)
Recent empirical studies across frontier agent benchmarks converge on key scientific principles:
1. **Interactive Rollouts over Static Artifacts** (Luo et al., 2025; Zhang et al., 2026; Chen et al., 2025; Foundation, 2026): Evaluation must test dynamic trajectory interaction with delayed side-effects and re-planning, not isolated snapshots.
2. **Hidden Holdouts & Leakage Prevention** (Kapoor & Narayanan, 2023; Xu et al., 2024; Zhou et al., 2023): Public-scenario tuning inflates scores; sealed hidden seeds and topology shifts are essential to verify true out-of-distribution generalization.
3. **Deterministic Governance over Prompt Controls** (Calboreanu, 2026; Shi et al., 2025; Costa et al., 2025; Uchibeke, 2026): Deterministic pre-action authorization, typed IR, and least-privilege containment are fundamentally more resilient than system-prompt guardrails.
4. **Process & Calibration Scoring** (Wang et al., 2026; Jang et al., 2026; Mei et al., 2025): Evaluating evidence closure, confidence calibration (ECE), and trajectory efficiency separates genuine causal inference from lucky heuristics.

### 10.2 The ESA-RBench 8-Stage Scenario Ladder

```text
  L8 [Long-Horizon Cascade]     --> 30+ step cascades, delayed consequences, multi-step re-planning
  L7 [Adversarial Pressure]     --> KPI conflict (profit vs SLA), prompt injection in webhooks
  L6 [Telemetry Corruption]     --> Contradictory metrics, stale sensor dropouts, timestamp drift
  L5 [Topology Shift]           --> Dynamic unseen payment rails & downstream dependency graphs
  L4 [Partial Observability]    --> Hidden bank rail health; zero traffic implies zero errors
  L3 [Interacting Faults]       --> Compute drop + bank degradation (thundering herd stress)
  L2 [Correlated Faults]        --> Surge burst + upstream gateway queue spillover
  L1 [Single Fault]             --> Isolated pod crash or localized bank rail 5xx spike
```

### 10.3 The 7 Meaningful Baselines & Privileged Reference Policy
To isolate causal capabilities, ESA-RBench evaluates:
- **B0 (Static Rules)**: Hardcoded metric thresholds.
- **B1 (Adaptive Scaler)**: Reactive HPA-style pod autoscaler (prone to thundering herd under bank degradation).
- **Classical RCA**: Distributed tracing and dependency graph causal inference without neural models.
- **Neuro-Symbolic Only**: ARC-AGI-3 Reasoner + DSL without background LLM.
- **LLM-Only (Advisory Mode)**: Generates diagnostic proposals for human operators without infrastructure tool execution credentials (0 unsafe executions by construction).
- **Ungated Tool-Using LLM**: LLM equipped with direct API mutation authority without policy verification or OCC, suffering catastrophic safety failures under prompt injections.
- **B2 (Full ESA)**: Governed autonomous runtime with ARC Reasoner + 3-tier cascade + deterministic policy gate + OCC token validation + rollback snapshots.
- **Oracle (Clairvoyant Reference Policy)**: Privileged reference policy with perfect internal state visibility operating within simulator physical action constraints (rate caps and step bounds).

### 10.4 Staged Path from Current Prototype to Production

| Stage | Evidence Standard | Credible Claim Boundary |
|---|---|---|
| **Current** | Internal simulator, 8 public scenarios, 155 synthetic runs | Promising governed prototype with verified safety primitives |
| **Research-Grade** | ESA-RBench 8-stage ladder, sealed hidden holdouts, ablations, statistical CI | Advanced autonomous reasoning & robust safety in simulated payment ops |
| **Pilot-Grade** | Passive shadow mode on real production telemetry, human-in-the-loop review | Operational diagnostic utility under operator supervision |
| **Production-Grade** | Gated low-risk autonomous remediation, external audit, 99.999% SLA window | Bounded autonomous remediation for pre-approved action classes |

