# Changelog

Meaningful milestones only — no fabricated history.

## v1.0.2 — Resilient Standalone Mode & Command Center Direct Serve (2026)

- **Universal CLI (`esapay-cli@1.0.2`)**: Autonomous client-side Standalone Evaluation Mode when offline; graceful fallbacks for `gateways`, `health`, `workloads`, `agents`, `checkout`, and `doctor`.
- **TypeScript SDK (`esapay@1.0.2`)**: Dual-mode ESM (`import`) and CommonJS (`require`) export mappings.
- **ESA Command Center at `/`**: Backend directly serves the live Command Center on `http://localhost:8080/` via same-origin static file service (`ServeDir`).
- **Transaction-Driven Telemetry & Spikes**: Real customer payments (`POST /api/payments/checkout`) drive live StateFabric workload metrics, P95 tail latency, queue depth, and sub-second failovers.
- **Custom Server Selector**: Connect Command Center to any remote cluster, local port, or Kubernetes service.

## v1.0.1 — Public Open Source Release (2026)

- Published `esapay-cli` and `esapay` to NPM Registry.
- Published `esapay` Python SDK to PyPI.
- Added GitHub Actions publishing pipelines and automated verification.

## v1.0 — Public Launch (2026)

- Full documentation set: architecture, governance, agents, benchmarks, claims register
- B0/B1/B2 benchmark harness with 155-trial matrix
- Command Center UI + payment simulator (Razorpay Test Mode)
- Audit hash chain, replay API, effect measurement
- Optional kubectl scale side effects

## v0.5 — Rollback

- Snapshot rollback via `ROLLBACK` action and gateway pre-execution snapshots
- BENCH-11 execution-failure scenario in harness

## v0.4 — Effect verification

- `EffectMeasurement` post-execution comparison
- Effects API endpoints

## v0.3 — Gateway / OCC

- Action Gateway as sole mutation path
- `RULE_003_STALE_STATE` and commit-time version check
- Stale-state demo scenario

## v0.2 — Agent loop

- Monitor, Diagnosis (Ollama), Planning, Safety agents
- Orchestrator 5s autonomous loop

## v0.1 — Initial runtime

- In-memory `StateFabric`
- Typed `ActionType` IR
- Policy engine skeleton
- `esa-api` health + workloads
