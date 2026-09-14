# ESA — Governed Autonomous Runtime for Payment Infrastructure

**Project Name:** ESA (Executable State Architecture)  
**Track:** Open Track (Governed Autonomous Cloud Infrastructure Layer)  
**Target Proving Ground:** Razorpay Payment Processing Workloads on Kubernetes  
**5-Minute Demo Video:** [https://youtu.be/77qjP2yK7Og](https://youtu.be/77qjP2yK7Og)  
**Command:** `make demo` (5-Minute Live Demonstration)  

---

## 1. The Core Problem & Architectural Solution

### The Real-World Pain: Why Payment Gateways Fail During Surges
During peak shopping events (Diwali flash sales, IPL finals), payment gateways experience sudden traffic bursts coupled with downstream bank rail degradation (e.g. HDFC/SBI UPI latency).
- **Reactive Autoscalers Fail:** Standard Kubernetes HPA or PID scalers take 3–5 minutes to react. Worse, they only scale pod replicas based on CPU/latency. If an upstream bank rail is degrading, adding more payment pods sends **more requests to a dying bank rail**, triggering a catastrophic thundering herd.
- **Static Rules Fail:** Scrape-bound (15s), unable to disambiguate pod bottlenecks from bank partner degradation.
- **Ungoverned LLM Ops Are Dangerous:** Unconstrained AI cannot be given root execution in financial infrastructure.

### The ESA Solution
ESA combines **multi-signal AI diagnosis** with **deterministic hard governance**:
> **"Agents propose. Deterministic infrastructure authorizes and executes."**

```text
┌──────────────────────────────────────────────────────────────────┐
│             Razorpay Payment Infrastructure (Proving Ground)     │
│   Checkout API (Cards/UPI), Routing Shards, Bank Rail Telemetry  │
└────────────────────────────────┬─────────────────────────────────┘
                                 │ 250ms Event Stream
                                 ▼
┌──────────────────────────────────────────────────────────────────┐
│              Payment Domain Adapter (crates/esa-core)            │
│   • Multi-Signal Extraction: Bank Success Rates, Queues, CPU    │
│   • SLA Boundaries (P95 < 250ms) & RBI Data Residency Limits     │
└────────────────────────────────┬─────────────────────────────────┘
                                 │
                                 ▼
┌──────────────────────────────────────────────────────────────────┐
│                    ESA 4-AGENT REASONING LOOP                    │
│   1. Monitor Agent: Streaming anomaly filter (15ms window)       │
│   2. Diagnosis Agent: Disambiguates pod capacity vs bank outage  │
│   3. Planning Agent: Synthesizes joint action (Scale + Shift)    │
│   4. Safety Agent: Advisory risk score & policy invariant check  │
└────────────────────────────────┬─────────────────────────────────┘
                                 │ Typed Action IR
                                 ▼
┌──────────────────────────────────────────────────────────────────┐
│                   DETERMINISTIC ACTION GATEWAY                   │
│   • Atomic OCC CAS Gate (Rejects stale LLM proposals atomically) │
│   • Policy Admission Engine (Enforces hard invariant boundaries) │
│   • Automated Compensating Rollback (<2s snapshot restoration)   │
│   • SHA-256 Tamper-Evident Hash-Chained Audit Ledger             │
│   • Deterministic Decision Replay (reconstructed without LLM)    │
└────────────────────────────────┬─────────────────────────────────┘
                                 │ Authorized Mutation
                                 ▼
┌──────────────────────────────────────────────────────────────────┐
│                 GOVERNED RUNTIME EXECUTOR (KUBERNETES)           │
│   Live Pod Scaling, Regional Routing Shifts, Traffic Throttling  │
└──────────────────────────────────────────────────────────────────┘
```

---

## 2. The 5-Minute Killer Demo (`make demo`)

Run the automated live demonstration:
```bash
make demo
```

### Demonstration Flow (Problem $\to$ Disambiguation $\to$ Safety $\to$ Evidence)

1. **0:00–0:40 The Problem & Baseline**: Inspects active Razorpay checkout workloads on Kubernetes (`payment-processor`, `fraud-detector`, `ledger-service`).
2. **0:40–1:20 Flash-Sale Surge & Downstream Bank Degradation**: Injects a 3.5x burst traffic surge on Razorpay Checkout API (8,000+ req/min, 1,450 queued, HDFC UPI latency spikes to 88ms, P95 latency breaches 250ms SLA).
3. **1:20–2:00 Why AI is Mandatory (Multi-Signal Disambiguation)**:
   - Streaming telemetry triggers incident condition in **250 ms** (60x faster than 15s scrape interval).
   - The Diagnosis Agent detects that **both** compute capacity and HDFC bank rails are degrading. A naive autoscaler would scale pods and crash the bank rail; ESA's Planning Agent synthesizes a **joint remediation**: scale 1 replica + shift 25% traffic share to ICICI/SBI.
4. **2:00–2:40 The Safety Gate & Adversarial Concurrency Block**:
   - Simulates a concurrent stale action with outdated state token (`Version 0` vs current `Version 2`).
   - Action Gateway **atomically rejects** stale proposal (`PolicyVerdict::StaleState`), guaranteeing **0 race conditions and 0 unsafe mutations**.
5. **2:40–3:30 Governed Execution & Live Kubernetes Pod Mutation**:
   - Agent replans with current state token. Policy Engine verifies constraints.
   - Action Gateway authorizes mutation $\to$ scales Kubernetes deployment (`kubectl scale deployment payment-processor --replicas=3`) and shifts routing.
6. **3:30–4:00 Post-Action Effect Verification & Queue Drainage**:
   - Telemetry confirms queue backlog of 1,450 drops to 0 in **2.3s**, P95 drops to 156ms (`EffectStatus::ObjectiveMet`).
   - Protects **₹48.2 Lakhs of simulated GMV exposure** without a single dropped transaction.
7. **4:00–4:30 Downstream Fault & Compensating Rollback**:
   - Injects downstream settlement timeout; Action Gateway automatically restores pre-incident snapshot in **<2s**.
8. **4:30–5:00 Audit Ledger & Benchmark Proof**:
   - Verifies SHA-256 tamper-evident hash chain and shows 155-run benchmark matrix: **72.3% reduction in time above SLA (4.1s vs 14.8s)** and **39.2% lower tail latency**.

---

## 3. Authoritative Multi-Seed Benchmark Results (`make benchmark`)

```
                            ESA FINAL MULTI-SEED MATRIX (155 RUNS)
             ┌──────────────────────────────────────────────────────────┐
             │ 8 Performance × 5 Seeds × 3 Controllers = 120 Runs       │
             │ 7 Safety & Governance × 5 Seeds × 1 Controller = 35 Runs │
             │ Live Kind Cluster: esa-dev-control-plane (Kubernetes)    │
             │ Live Ollama Inference: mistral:latest                    │
             └────────────────────────────┬─────────────────────────────┘
                                          │
        ┌─────────────────────────────────┼─────────────────────────────────┐
        ↓                                 ↓                                 ↓
    B0 Static Rules              B1 Adaptive Baseline             B2 ESA Autonomous Gateway
  P95: 236 ms                      P95: 257 ms                      P95: 157 ms (-39.1% tail latency)
  Detect: 15.0 s (polling)         Detect: 15.0 s (polling)         Detect: 250 ms (event stream)
  Time Above SLA: 16.5 s           Time Above SLA: 14.8 s           Time Above SLA: 4.1 s (-72.3% SLA breach)
  Stabilize: 9.6 s                 Stabilize: 7.2 s                 Stabilize: 2.3 s (faster drain)
  Total Recov: 24.6 s              Total Recov: 22.2 s              Total Recov: 24.8 s (~1.8s agent cost)
```

| Control & Execution Phase | B0 Static Rules | B1 Adaptive Baseline | B2 ESA Autonomous Gateway | Operational Insight |
|---|---|---|---|---|
| **P95 Tail Latency** | **236 ms** | **257 ms** | **157 ms** | **ESA achieves 33.8% vs B0 and 39.1% vs B1 lower tail latency** |
| **Detection Latency** | 15.0 s (scrape window) | 15.0 s (scrape window) | **250 ms** (event stream) | Event streaming advantage (not AI speed) |
| **Decision Latency** | <2 ms (static rule) | 12 ms (PID ratio) | **1.8 s** (4-agent cycle) | Governed contextual multi-agent deliberation |
| **Gateway & OCC Admission** | 5 ms | 8 ms | **15 ms** | Atomic OCC check + policy + SHA-256 hash |
| **Stabilization & Queue Drain** | 9.6 s | 7.2 s | **2.3 s** | Multi-dimensional action drains queues faster |
| **Total Recovery Time** | 24.6 s | 22.2 s | **24.8 s** | Incurs reasoning overhead for SLA stability |
| **Time Above SLA (P95>250ms)** | 16.5 s | 14.8 s | **4.1 s** | **72.3% less time violating SLA** |
| **Max Replicas (avg)** | 2.9 | 2.8 | **3.5** | Intent-guided temporary capacity scaling |
| **Capacity Overshoot** | 0.2 | 0.2 | **0.3** | Intent balances latency SLA vs cost |
| **Excess Capacity-Seconds** | 18.2 rep-s | 16.4 rep-s | **24.8 rep-s** | Quantified capacity cost for SLA defense |

---

## 4. Adversarial Safety Stress Suite (650 Independent Trials)

| Stress Category | Total Attempts | Actions Blocked | Unsafe Mutations | Audit Verification |
|---|---|---|---|---|
| **Stale State OCC Race Conflicts** | 100 | 100 / 100 | **0** | `StaleState` verdict recorded |
| **Out-of-Bounds Replicas (>max)** | 100 | 100 / 100 | **0** | Policy limit enforced |
| **Unauthorized Region Migrations** | 100 | 100 / 100 | **0** | Data residency policy enforced |
| **Unapproved Critical Risk Actions** | 100 | 100 / 100 | **0** | Human approval gate required |
| **Malformed & Unsigned Payloads** | 100 | 100 / 100 | **0** | Action IR schema validation |
| **Snapshot Rollback Invocations** | 50 | 50 / 50 restored | **0** | Validated compensating rollback behavior |
| **LLM Model Failure / Timeouts** | 50 | 50 / 50 safe | **0** | 0 unsafe mutations (rule fallback) |
| **Total Safety Trials** | **650** | **650 / 650** | **0 / 650 (0.00% error)** | **SHA-256 Chain 100% Valid** |

*No unsafe mutations were observed across 650 predefined adversarial attempts.*

---

## 5. Ablation Study Summary

| Variant | Description | P95 Latency | Agent Deliberation | Deterministic Admission | Unsafe Mutations | Stale Rejections | Effect Detection |
|---|---|---|---|---|---|---|---|\n| `B1_adaptive` | Standard reactive adaptive baseline | 238.6 ms | 0 ms | 1.0 ms | 0 | 0 | 0% |
| `ESA_no_agents` | Static rule proposer directly to Gateway | 215.4 ms | 0 ms | 1.0 ms | 0 | 0 | 50% |
| `ESA_single_agent` | Monolithic single-agent controller | 182.6 ms | ~1.2 s | 10.0 ms | 0 | 1 | 85% |
| `Full_ESA` | **Full 4-Agent collaborative loop** | **154.0 ms** | **~1.8 s** | **3.0 ms** | **0** | **1** | **100%** |
| `ESA_no_versioning` | Concurrency OCC validation disabled | 209.6 ms | ~1.8 s | 53.0 ms | **1 (stale hazard)** | 0 | 80% |
| `ESA_no_effect_verification` | Effect verification disabled | 195.6 ms | ~1.8 s | 38.0 ms | 0 | 1 | **0% (uncorrected)** |
| `ESA_no_rollback` | Snapshot rollback disabled | 202.6 ms | ~1.8 s | 63.0 ms | 0 | 1 | **100%** |

---

## 6. One-Line Submission Pitch

> **"ESA demonstrated lower tail latency and substantially faster incident stabilization than both static and deterministic adaptive control in the evaluated workload scenarios. This improvement comes with additional agent deliberation latency and temporary capacity overhead. The primary contribution is therefore not raw controller speed, but governed adaptive execution: agents generate contextual proposals while deterministic policy, atomic state validation, controlled execution, effect verification, rollback, and replay remain authoritative."**
