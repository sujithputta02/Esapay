# Ablation study

Ablation variants illustrate **why each architectural layer matters**. Read this before citing §7 of `benchmarkreport.md`.

## Live trials (3 variants)

These run real harness trials on **BENCH-02** burst with seed `481923`:

| Variant | Controller / path | What is real |
|---------|-------------------|--------------|
| `B1_adaptive` | `Controller::B1` | Full B1 trial metrics |
| `ESA_no_agents` | `Controller::B0` | Static rules → gateway (no LLM agents) |
| `Full_ESA` | `Controller::B2` | Full 4-agent + gateway + OCC + effects |

Implementation: `run_ablation_study()` in `benchmark_harness.rs`.

## Modeled offsets (4 variants)

These variants **do not disable feature flags**. Metrics are **arithmetic offsets** from the `Full_ESA` trial:

| Variant | Modeled behavior |
|---------|------------------|
| `ESA_single_agent` | P95 +15ms, recovery ×0.95, synthetic stale_rej=1, effect=85% |
| `ESA_no_versioning` | P95 +42ms, recovery +50ms, unsafe_mutations=1 |
| `ESA_no_effect_verification` | P95 +28ms, effect_detection=0% |
| `ESA_no_rollback` | P95 +35ms, recovery +60ms |

**Label in claims register:** Partial — see `docs/claims.md`.

## Interpretation guide

| Removed component | Expected degradation (conceptual) | Evidence in repo |
|-------------------|-----------------------------------|------------------|
| Agents (→ B0 path) | Less contextual actions | Live `ESA_no_agents` trial |
| Multi-agent (single) | Less review, stale hazard | **Modeled offset only** |
| OCC versioning | Stale writes possible | **Modeled offset only**; live tests in `atomic_concurrency_test` |
| Effect verification | Uncorrected underperformance | **Modeled offset**; live `EffectMeasurement` in gateway |
| Rollback | Failed mutations stick | **Modeled offset**; live `rollback_test`, BENCH-11 |

## Reproduce

```bash
cargo run --bin esa-benchmark -- --ablations
# or
curl -X POST http://localhost:8080/api/benchmark/ablations
```

Output: `benchmarks/processed/ablations.json`

## Future work

Per-variant feature flags to run **live** ablations without arithmetic offsets.
