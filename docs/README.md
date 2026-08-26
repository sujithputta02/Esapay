# ESA Documentation

Project documentation lives here. The root [`benchmarkreport.md`](../../benchmarkreport.md) stays at repo root for quick access to latest benchmark results.

## Start here

| Document | Description |
|----------|-------------|
| [QUICK_START.md](QUICK_START.md) | 5-minute demo flow (Command Center, simulator, scenarios) |
| [DEMO_GUIDE.md](DEMO_GUIDE.md) | Judge pitch script and live demo narrative |
| [QUICKSTART.md](QUICKSTART.md) | Docker-based full stack setup |

## Product & requirements

| Document | Description |
|----------|-------------|
| [ESA_paymentprdv2.md](ESA_paymentprdv2.md) | Product requirements (PRD v2) |
| [PRD_IMPLEMENTATION_CHECKLIST.md](PRD_IMPLEMENTATION_CHECKLIST.md) | Feature checklist vs PRD |
| [design_skills.md](design_skills.md) | Design notes and agent skills context |

## Status & verification

| Document | Description |
|----------|-------------|
| [PROJECT_STATUS.md](PROJECT_STATUS.md) | Overall project status |
| [DEMO_STATUS.md](DEMO_STATUS.md) | Demo readiness snapshot |
| [DEMO_READY.md](DEMO_READY.md) | Demo go-live notes |
| [DEMO_READINESS_REPORT.md](DEMO_READINESS_REPORT.md) | Readiness audit |
| [IMPLEMENTATION_SUMMARY.md](IMPLEMENTATION_SUMMARY.md) | Implementation summary |
| [FINAL_PROJECT_SUMMARY.md](FINAL_PROJECT_SUMMARY.md) | Final project summary |
| [FINAL_VERIFICATION_REPORT.md](FINAL_VERIFICATION_REPORT.md) | Verification report |
| [ESA_FINAL_VERIFICATION_REPORT.md](ESA_FINAL_VERIFICATION_REPORT.md) | ESA-specific verification |

## Benchmarks

- Latest report: [`../benchmarkreport.md`](../benchmarkreport.md)
- Raw data: [`../benchmarks/raw/benchmark_results.json`](../benchmarks/raw/benchmark_results.json)
- Scenario taxonomy: [`../benchmarks/scenarios/taxonomy.yaml`](../benchmarks/scenarios/taxonomy.yaml)

Run benchmarks:

```bash
make benchmark-quick    # smoke (~4s)
make benchmark-smoke    # full agent cycle, 1 seed
make benchmark          # full 5-seed suite
```
