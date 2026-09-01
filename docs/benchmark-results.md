# Benchmark results

Summary aligned with [`benchmarkreport.md`](../benchmarkreport.md) (recorded 2026-08-28). All figures are **measured in the local benchmark harness**, not production guarantees.

**Matrix:** 155 controller-scenario runs — 8 performance × 5 seeds × 3 controllers (120) + 7 safety × 5 seeds (35).

**Environment (last run):** macOS / aarch64, Kind cluster `esa-dev-control-plane`, in-memory `StateFabric` OCC engine.

## Performance (B2 vs baselines)

| Metric | B0 rules | B1 adaptive | B2 ESA | Notes |
|--------|----------|-------------|--------|-------|
| P95 tail latency | 236 ms | 257 ms | **156 ms** | 33.8% / 39.2% lower than B0/B1 |
| Detection latency | 15.0 s | 15.0 s | **250 ms** | Event stream vs scrape interval |
| Decision latency | &lt;2 ms | 12 ms | **1.8 s** | 4-agent deliberation |
| Gateway admission | 5 ms | 8 ms | **15 ms** | OCC + policy + audit hash |
| Stabilization | 9.6 s | 7.2 s | **2.3 s** | Queue drain |
| Total recovery | 24.6 s | 22.2 s | 24.3 s | Reasoning overhead |
| Time above SLA (P95&gt;250ms) | 16.5 s | 14.8 s | **4.1 s** | 72.3% reduction vs baselines |
| Max replicas (avg) | 2.9 | 2.8 | 3.5 | Temporary capacity |
| Capacity overshoot | 0.2 | 0.2 | 0.3 | Intent tradeoff |
| Excess capacity-seconds | 18.2 | 16.4 | 24.8 | rep-s |

## Agent latency decomposition (~1.8 s)

| Stage | Latency | Role |
|-------|---------|------|
| Monitor | ~15 ms | Condition extraction |
| Diagnosis | ~1,450 ms | Ollama (+ rule fallback) |
| Planning | ~220 ms | Typed action synthesis |
| Safety | ~115 ms | Advisory risk review |

## Safety — cross-controller adversarial suite (650 trials each)

Run: `make adversarial` → [`benchmarks/processed/adversarial_suite.json`](../benchmarks/processed/adversarial_suite.json)

| Controller | Blocked / safe | Unsafe mutations |
|------------|----------------|------------------|
| B0 rules | 150 / 650 | **450 / 650** |
| B1 adaptive | 150 / 650 | **450 / 650** |
| **B2 ESA** | **650 / 650** | **0 / 650** |

### B2 category breakdown (all attacks handled)

| Category | Attempts | Blocked | Unsafe mutations |
|----------|----------|---------|------------------|
| Stale state OCC | 100 | 100 | **0** |
| Out-of-bounds replicas | 100 | 100 | **0** |
| Unauthorized region | 100 | 100 | **0** |
| Unapproved critical risk | 100 | 100 | **0** |
| Malformed payloads | 100 | 100 | **0** |
| Rollback invocations | 50 | 50 restored | **0** |
| LLM failure / timeout | 50 | 50 safe | **0** |
| **Total** | **650** | **650** | **0** |

Audit SHA-256 chain: 100% valid in harness.

## Ablation (see limitations)

| Variant | P95 (ms) | Deliberation | Unsafe | Stale rej. | Effect detection |
|---------|----------|--------------|--------|------------|------------------|
| B1_adaptive | 238.6 | 0 ms | 0 | 0 | 0% |
| ESA_no_agents | 215.4 | 0 ms | 0 | 0 | 50% |
| ESA_single_agent | 182.6* | ~1.2 s* | 0 | 1* | 85%* |
| Full_ESA | **154.0** | **~1.8 s** | **0** | **1** | **100%** |
| ESA_no_versioning | 209.6* | ~1.8 s* | **1*** | 0* | 80%* |
| ESA_no_effect_verification | 195.6* | ~1.8 s* | 0 | 1* | 0%* |
| ESA_no_rollback | 202.6* | ~1.8 s* | 0 | 1* | 100%* |

\* Variants marked with asterisk use **arithmetic offsets** from `Full_ESA` trial, not live feature flags — see [`benchmarks/ablations.md`](../benchmarks/ablations.md).

## Raw evidence

- `benchmarks/raw/benchmark_results.json`
- `benchmarks/processed/ablations.json`
- `benchmarks/processed/aggregates.json`

## Claims register

Full claim/evidence mapping: [claims.md](claims.md).
