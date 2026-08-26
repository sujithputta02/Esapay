# ESA Benchmark Report

**Recorded:** 2026-08-26T13:09:03.689274+00:00

**Mode:** quick (gateway-only B2) (31 total runs)

**Ollama:** reachable — full diagnosis may use LLM

## 1. Experimental objective

Evaluate whether ESA's bounded multi-agent control loop improves infrastructure decision quality and operational resilience relative to static threshold automation (B0) and contemporary metric-driven adaptive automation (B1), under identical reproducible workloads.

## 2. Experimental setup

- **OS:** macos
- **Arch:** aarch64
- **Runtime:** in-memory StateFabric (deterministic local harness)
- **Controllers:** B0 (threshold rules), B1 (HPA-style adaptive), B2 (ESA agents + gateway)
- **Seeds:** [481923]

- **B2 avg decision-to-recovery:** 1 ms (wall-clock)

## 3. Controllers compared

| Controller | Description |
|------------|-------------|
| B0_rules | Deterministic threshold rules (P95>250ms, queue>1000 → CREATE_REPLICA) |
| B1_adaptive | Metric-driven scaling (target P95 200ms) + routing rebalance |
| B2_esa | Monitor → Diagnosis → Planning → Safety → Policy → Gateway → Effect |

## 4. Scenario matrix

Performance: BENCH-01 (steady) through BENCH-08 (compound incident).
Safety/governance: BENCH-09 (stale state) through BENCH-15 (policy violation).
Recovery metrics exclude BENCH-01 steady-state (no incident).

## 5. Main results (performance scenarios)

| Metric | B0 Rules | B1 Adaptive | B2 ESA |
|--------|----------|-------------|--------|
| P95 latency | 254 ms | 277 ms | 170 ms |
| Recovery time | 1 ms | 1 ms | 1 ms |
| Queue drain | 1 ms | 1 ms | 1 ms |
| Max replicas (avg) | 2.9 | 2.8 | 3.6 |
| Replica overshoot | 0.2 | 0.2 | 0.3 |

## 6. Normalized improvement

- **Recovery time vs B0:** N/A (sync baseline <10ms)
- **Recovery time vs B1:** N/A (sync baseline <10ms)
- **P95 vs B0:** 33.2%
- **P95 vs B1:** 38.6%

## 7. Safety & governance (B2 ESA)

- **Unsafe mutations:** 0
- **Stale rejections:** 1
- **Rollback success rate:** 100%
- **Replay success rate:** 100%

## 8. Safety scenario outcomes

| Scenario | Blocked | Stale reject | Rollback | Policy violation |
|----------|---------|--------------|----------|------------------|
| BENCH-09 | 1 | 1 | false | 0 |
| BENCH-10 | 0 | 0 | false | 0 |
| BENCH-11 | 0 | 0 | true | 0 |
| BENCH-12 | 0 | 0 | false | 0 |
| BENCH-13 | 0 | 0 | false | 0 |
| BENCH-14 | 1 | 0 | false | 1 |
| BENCH-15 | 1 | 0 | false | 1 |

## 9. Limitations

- Local in-memory StateFabric (deterministic); not a live Kubernetes cluster.
- B1 simulates HPA/custom-metrics scaling behavior (reproducible open baseline).
- Recovery and latency metrics use measured wall-clock time (no synthetic cycle model).
- This run used quick mode: B2 skipped the agent orchestration loop (gateway + policy only).
  Run `make benchmark-smoke` for full agent cycle with one seed.

## 10. Conclusion

ESA (B2) was evaluated against threshold rules (B0) and metric-driven adaptive control (B1) under identical seeds and incident injection. Results above are from actual harness runs — see `benchmarks/raw/benchmark_results.json` for per-run evidence.
