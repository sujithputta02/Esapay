# ESA Documentation

Engineering and research documentation for the ESA + Razorpay Buildathon prototype.

**Rule:** All claims must match [claims.md](claims.md) and [../benchmarkreport.md](../benchmarkreport.md).

## Architecture & execution

| Document | Description |
|----------|-------------|
| [architecture.md](architecture.md) | Layered system design |
| [execution-flow.md](execution-flow.md) | End-to-end control loop |
| [agent-model.md](agent-model.md) | Agent vs gateway authority |
| [governance.md](governance.md) | Policy, typed IR, gateway pipeline |
| [state-management.md](state-management.md) | OCC, snapshots, stale proposals |
| [failure-recovery.md](failure-recovery.md) | Failure modes and responses |
| [effect-verification.md](effect-verification.md) | Expected vs observed effects |
| [audit-replay.md](audit-replay.md) | Hash chain, replay, provenance |

## Agents

| Document | Description |
|----------|-------------|
| [agents/monitor.md](agents/monitor.md) | Monitor agent |
| [agents/diagnosis.md](agents/diagnosis.md) | Diagnosis agent |
| [agents/planning.md](agents/planning.md) | Planning agent |
| [agents/safety.md](agents/safety.md) | Safety agent |

## Operations & API

| Document | Description |
|----------|-------------|
| [demo.md](demo.md) | Judge / operator demo manual |
| [reproducibility.md](reproducibility.md) | Clone → benchmark path |
| [api.md](api.md) | HTTP endpoints (implemented only) |
| [benchmark-results.md](benchmark-results.md) | Results summary |
| [claims.md](claims.md) | **Claims register** |

## Benchmarks folder

| Document | Description |
|----------|-------------|
| [../benchmarks/README.md](../benchmarks/README.md) | Harness overview |
| [../benchmarks/methodology.md](../benchmarks/methodology.md) | Measurement setup |
| [../benchmarks/scenarios.md](../benchmarks/scenarios.md) | BENCH-01–15 |
| [../benchmarks/baselines.md](../benchmarks/baselines.md) | B0 / B1 / B2 |
| [../benchmarks/ablations.md](../benchmarks/ablations.md) | Ablation limitations |
| [../benchmarkreport.md](../benchmarkreport.md) | Latest full report |

## Product requirements (PRD)

| Document | Description |
|----------|-------------|
| [ESA_paymentprdv2.md](ESA_paymentprdv2.md) | Product requirements (PRD v2) |
| [PRD_IMPLEMENTATION_CHECKLIST.md](PRD_IMPLEMENTATION_CHECKLIST.md) | PRD feature checklist |

## Repo meta

- [../SECURITY.md](../SECURITY.md)
- [../CONTRIBUTING.md](../CONTRIBUTING.md)
- [../CHANGELOG.md](../CHANGELOG.md)
- [../LICENSE](../LICENSE)
