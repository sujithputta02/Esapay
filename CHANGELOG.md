# Changelog

Meaningful milestones only — no fabricated history.

## v1.0 — Buildathon submission (2026)

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
