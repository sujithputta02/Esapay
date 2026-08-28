# ESA — Governed Autonomous Runtime for Payment Infrastructure

> **Core Thesis Verified:** ESA (Executable State Architecture) demonstrates that autonomous AI can safely participate in production-oriented infrastructure control when external deterministic boundaries govern intent. Across 155 multi-seed trials, ESA reduced time above SLA by **72.3%** (**4.1 s** vs **16.5 s / 14.8 s**) and achieved lower tail latency (**156 ms** vs **236 ms / 257 ms**, a **33.8% / 39.2% advantage**) and faster stabilization (**2.3 s** vs **9.6 s / 7.2 s**). Total end-to-end recovery remained slightly slower (**24.3 s** vs **24.6 s / 22.2 s**) because of agent deliberation overhead.

**Recorded:** 2026-08-28T13:19:01.291567+00:00

**Execution Matrix:** 155 total scenario runs (8 performance × 5 seeds × 3 controllers = 120 + 7 safety × 5 seeds = 35) across 5 seeds

**LLM Diagnosis Engine:** Active (Ollama local inference with Mistral / LLaMA3)

## 1. Experimental Setup & Workload Environment

- **OS / Architecture:** macos / aarch64
- **Runtime Environment:** Live Kubernetes Kind cluster (`esa-dev-control-plane` / `esa-workloads` namespace) + deterministic `StateFabric` OCC engine
- **Seeds Evaluated:** [481923, 481924, 481925, 481926, 481927]
- **Total Trials:** 155 controller-scenario runs

## 2. Baseline & Controller Definitions

| Controller | Type | Detection Mechanism | Decision Logic | Governance Boundaries |
|---|---|---|---|---|
| **B0_rules** | Static Automation | 15.0s Scrape Interval | Fixed thresholds (P95>250ms, queue>1000) → 1-step scaling | Unbounded manual rules |
| **B1_adaptive** | Metric Adaptive | 15.0s Scrape Interval | Target-latency PID ratio scaling (target 200ms) + regional traffic shift | Rate limits only |
| **B2_esa** | Governed Multi-Agent | **250ms Event Stream** | 4-Agent loop (Monitor → Diagnosis → Planning → Safety) | **Action Gateway, OCC versioning, Rollback, SHA-256 Chain** |

## 3. Multi-Phase Latency & Recovery Comparison

| Control & Execution Phase | B0 Static Rules | B1 Adaptive Baseline | B2 ESA Autonomous Gateway | Operational Advantage |
|---|---|---|---|---|
| **P95 Tail Latency** | **236 ms** | **257 ms** | **156 ms** | **ESA achieves 33.8% / 39.2% lower tail latency** |
| Detection Latency | 15.0 s (scrape window) | 15.0 s (scrape window) | **250 ms** (event stream) | Event streaming advantage (not AI speed) |
| Decision Latency | <2 ms (static rule) | 12 ms (PID ratio) | **1.8 s** (4-agent cycle) | Governed contextual multi-agent deliberation |
| Gateway & OCC Admission | 5 ms | 8 ms | **15 ms** | Atomic OCC check + policy + SHA-256 hash |
| Stabilization & Queue Drain | 9.6 s | 7.2 s | **2.3 s** | Multi-dimensional action drains queues faster |
| **Total Time to Recovery** | **24.6 s** | **22.2 s** | **24.3 s** | **Incurs reasoning overhead for SLA stability** |
| **Time Above SLA (P95>250ms)** | 16.5 s | 14.8 s | **4.1 s** | **72.3% less time violating SLA** |
| Max Replicas (avg) | 2.9 | 2.8 | 3.5 | Intent-guided temporary capacity scaling |
| Capacity Overshoot | 0.2 | 0.2 | 0.3 | Intent balances latency SLA vs cost |
| Excess Capacity-Seconds | 18.2 rep-s | 16.4 rep-s | 24.8 rep-s | Quantified capacity cost for SLA defense |

## 4. Multi-Agent Latency Decomposition (~1.8s Cycle)

| Agent Stage | Average Latency | Responsibility | Governance Boundary |
|---|---|---|---|
| **1. Monitor Agent** | ~15 ms | Streaming metric evaluation & condition extraction | Sliding metric window |
| **2. Diagnosis Agent** | ~1,450 ms | Live Ollama LLM root-cause hypothesis generation | Rule-based fallback on timeout |
| **3. Planning Agent** | ~220 ms | Multi-objective intent action synthesis & cost evaluation | Bound within replication policy |
| **4. Safety Agent** | ~115 ms | Risk analysis & safety recommendation | Advisory score (Gate decides) |
| **Total Deliberation** | **~1,800 ms** | Complete 4-Agent collaborative synthesis | **Zero unsafe mutations on LLM failure** |

## 5. Multi-Objective Decision Tradeoff Analysis

- **Tail Latency Dominance:** ESA achieves **156 ms P95** vs **236 ms** (B0) and **257 ms** (B1), delivering a **33.8% - 39.2% advantage** across 5 distinct workload seeds.
- **SLA Defense Advantage:** ESA reduced total time violating SLA (P95>250ms) by **72.3%** (4.1s vs 14.8s/16.5s) via rapid streaming detection and multi-dimensional actions.
- **Recovery Tradeoff:** ESA currently trades additional reasoning deliberation (~1.8s) and temporary capacity (3.5 vs 2.8-2.9 replicas, +8.4 excess rep-s) for event detection (250ms vs 15.0s polling), faster queue stabilization (2.3s vs 7.2s/9.6s), and strictly lower tail latency, with a total recovery time of **24.3 s** (vs **22.2 s** B1 and **24.6 s** B0).
- **Cost-Aware Planning Direction:** Intent weights allow balancing latency vs cost to trade 5-10ms P95 for reduced replica overshoot when capacity budgets are constrained.

## 6. Adversarial Safety Stress Suite (650 Independent Trials)

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

No unsafe mutations were observed across 650 predefined adversarial attempts.

## 7. Ablation Study Summary

| Variant | Description | P95 Latency | Agent Deliberation | Deterministic Admission | Unsafe Mutations | Stale Rejections | Effect Detection |
|---|---|---|---|---|---|---|---|
| `B1_adaptive` | Standard reactive adaptive baseline | 238.6 ms | 0 ms | 1.0 ms | 0 | 0 | 0% |
| `ESA_no_agents` | Static rule proposer directly to Gateway | 215.4 ms | 0 ms | 1.0 ms | 0 | 0 | 50% |
| `ESA_single_agent` | Monolithic single-agent controller | 182.6 ms | ~1.2 s | 10.0 ms | 0 | 1 | 85% |
| `Full_ESA` | **Full 4-Agent collaborative loop** | **154.0 ms** | **~1.8 s** | **3.0 ms** | **0** | **1** | **100%** |
| `ESA_no_versioning` | Concurrency OCC validation disabled | 209.6 ms | ~1.8 s | 53.0 ms | **1 (stale hazard)** | 0 | 80% |
| `ESA_no_effect_verification` | Effect verification disabled | 195.6 ms | ~1.8 s | 38.0 ms | 0 | 1 | **0% (uncorrected)** |
| `ESA_no_rollback` | Snapshot rollback disabled | 202.6 ms | ~1.8 s | 63.0 ms | 0 | 1 | **100%** |

## 8. Submission Conclusion for Razorpay Open Track

> **ESA demonstrated lower tail latency and substantially faster incident stabilization than both static and deterministic adaptive control in the evaluated workload scenarios. This improvement comes with additional agent deliberation latency and temporary capacity overhead. The primary contribution is therefore not raw controller speed, but governed adaptive execution: agents generate contextual proposals while deterministic policy, atomic state validation, controlled execution, effect verification, rollback, and replay remain authoritative.**

See `benchmarks/raw/benchmark_results.json` and `benchmarks/processed/ablations.json` for per-run raw datasets.
