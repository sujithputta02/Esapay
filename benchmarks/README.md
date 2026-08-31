# ESA Benchmarks

Deterministic harness comparing three controllers on identical payment-workload scenarios.

## Why benchmark?

ESA claims **governed adaptive execution** — not raw controller speed. The harness measures:

- Tail latency and time above SLA
- Detection vs decision vs stabilization phases
- Safety boundaries (stale state, policy, rollback, LLM failure)
- Ablation of architectural components (partial)

## Controllers

| ID | Label | Description |
|----|-------|-------------|
| **B0** | `B0_rules` | Static thresholds (P95&gt;250ms, queue&gt;1000) → single-step scale |
| **B1** | `B1_adaptive` | HPA-style target-latency ratio scaling (target 200ms) + regional shift |
| **B2** | `B2_esa` | Full ESA: Monitor → Diagnosis → Planning → Safety → Gateway |

## Scenarios

- **8 performance:** BENCH-01 through BENCH-08 (`benchmarks/scenarios/taxonomy.yaml`)
- **7 safety:** BENCH-09 through BENCH-15

## Seeds

5 fixed seeds: `481923` … `481927` (reproducible RNG).

## Metrics

Per trial: P95, detection/decision/gateway latency, stabilization, total recovery, time above SLA, replica usage, capacity overshoot, unsafe mutations, policy outcomes.

Aggregated in `benchmarks/processed/aggregates.json` and `benchmarkreport.md`.

## Reproduce

```bash
make benchmark-quick      # smoke
make benchmark-smoke      # 1 seed, full agent path
make benchmark            # full matrix
cargo run --bin esa-benchmark -- --ablations
```

Requires: Rust workspace, in-process `StateFabric` + orchestrator (Ollama optional for B2 diagnosis).

## Outputs

| Directory | Contents |
|-----------|----------|
| `raw/` | `benchmark_results.json` |
| `processed/` | `aggregates.json`, `ablations.json` |
| `reports/` | Generated markdown report |
| `scenarios/` | `taxonomy.yaml` |

## Documentation

- [methodology.md](methodology.md)
- [scenarios.md](scenarios.md)
- [baselines.md](baselines.md)
- [ablations.md](ablations.md)
- [../docs/benchmark-results.md](../docs/benchmark-results.md)
- [../benchmarkreport.md](../benchmarkreport.md)

## Scope

Results are **validated in the benchmark environment** (local Kind + in-memory state). Not production GMV or settlement guarantees.
