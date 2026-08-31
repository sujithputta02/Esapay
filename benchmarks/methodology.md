# Benchmark methodology

How the ESA harness measures controllers (`crates/esa-api/src/benchmark_harness.rs`, `benchmark.rs`, `bin/esa-benchmark.rs`).

## Hardware / OS

Recorded per run in `benchmarkreport.md` §1 (e.g. macOS aarch64). Re-run on your machine for fresh numbers.

## Kubernetes

Optional Kind cluster (`esa-dev-control-plane`, namespace `esa-workloads`) for gateway kubectl side effects. Core metrics come from **deterministic `StateFabric` simulation**.

## Runtime

- In-memory `StateFabric` with OCC versioning
- `EsaOrchestrator` for B2 controller path
- `ActionGateway` for all B2 typed mutations

## LLM

- **B2:** Ollama local inference (`OLLAMA_URL`, `OLLAMA_MODEL`)
- Diagnosis uses LLM with **rule-based fallback** on timeout/failure
- B0/B1 do not invoke agents

## Seeds

Fixed list in `taxonomy.yaml`: `[481923, 481924, 481925, 481926, 481927]`.

Each trial seeds workload RNG / scenario applicators deterministically.

## Traffic generation

Scenarios mutate workload metrics (rate, P95, queue, replicas, region skew) via harness applicators — not live production traffic.

Performance matrix default: **8 scenarios × 5 seeds × 3 controllers = 120** runs.

Safety matrix: **7 scenarios × 5 seeds = 35** runs.

## Scenario definitions

See [scenarios.md](scenarios.md) and `scenarios/taxonomy.yaml`.

## Measurement methodology

| Phase | B0/B1 | B2 |
|-------|-------|-----|
| Detection | 15s scrape window modeled | 250ms event-stream window |
| Decision | Rule / PID compute | Wall-clock agent cycle (~1.8s avg decomposed) |
| Gateway | Simulated admission ms | Policy + OCC + audit timing |
| Stabilization | Time until P95 &lt; SLA and queue drained | Same criteria |
| Recovery | Wall-clock end-to-end | Includes deliberation overhead |

Effectiveness scored via `EffectMeasurement::calculate` when gateway executes.

## Controllers

Documented in [baselines.md](baselines.md).

## Ablation

See [ablations.md](ablations.md) — three variants run live trials; four use modeled offsets.

## Commands

```bash
make benchmark-quick       # --quick
make benchmark-smoke       # --smoke-full
make benchmark             # default full run
cargo run --bin esa-benchmark -- --ablations
```

API alternative: `POST /api/benchmark/harness`, `POST /api/benchmark/ablations`.

## Verification

```bash
make audit-verify
cargo test --workspace
```
