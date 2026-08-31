# ESA Claims Register

Every major claim in README, demo scripts, and benchmark reports should match this table.

| Claim | Evidence | Status |
|-------|----------|--------|
| 4-agent bounded control loop (Monitor → Diagnosis → Planning → Safety → Gateway) | `crates/esa-runtime/src/orchestrator.rs` | **Implemented** |
| Agents propose; gateway executes | Gateway is sole mutation path | **Implemented** |
| Typed actions only (`CREATE_REPLICA`, `SHIFT_ROUTE`, `ROLLBACK`, …) | `crates/esa-core/src/actions.rs` | **Implemented** |
| Policy engine (ALLOW / DENY / STALE / REQUIRES_APPROVAL) | `crates/esa-policy` | **Implemented** |
| Stale-state rejection (OCC) | `RULE_003_STALE_STATE`, gateway commit check | **Implemented** |
| Snapshot rollback on execution failure | `ActionGateway::apply_rollback` | **Implemented** |
| Effect measurement (ObjectiveMet / Underperformed / Failed) | `EffectMeasurement::calculate` | **Implemented** |
| SHA-256 audit hash chain | `AuditStore::append` | **Implemented** (in-memory) |
| Audit chain verification API | `GET /api/audit/verify-chain` | **Implemented** |
| Decision replay (deterministic, no LLM) | `DecisionReplayer`, replay endpoints | **Implemented** |
| Razorpay Test Mode orders + webhooks | `esa-razorpay`, payment simulator | **Implemented** |
| B0 / B1 / B2 benchmark harness | `esa-benchmark`, `make benchmark*` | **Implemented** |
| P95 ~156 ms vs B0 ~236 ms (5 seeds) | `benchmarkreport.md`, raw JSON | **Measured in harness** |
| Time above SLA ~4.1 s vs B0 ~16.5 s | `benchmarkreport.md` | **Measured in harness** |
| 650 safety trials, 0 unsafe mutations | `benchmarkreport.md` §6 | **Measured in harness** |
| Optional `kubectl scale` on replica changes | `sync_to_k8s_deployment` when `KUBERNETES_ENABLED` | **Implemented (optional)** |
| Autonomous loop every 5 seconds | `orchestrator.run_forever(5s)` | **Implemented** |
| Ollama diagnosis with rule fallback | `diagnosis.rs` | **Implemented** |
| PostgreSQL-backed state fabric | `StateStore` exists | **NOT wired to API** |
| Redis / NATS in runtime | Compose only | **NOT wired** |
| Prometheus `/metrics` scrape | No `/metrics` route on API | **NOT implemented** |
| Automatic replan on failed effect | Not in orchestrator loop | **NOT implemented** |
| Full ablation with live feature flags | 3 variants use modeled offsets | **Partial — see `benchmarks/ablations.md`** |
| Production deployment | — | **NOT CLAIMED** |
| RBI / PCI compliance | — | **NOT CLAIMED** |
| Real GMV protection / settlement | — | **NOT CLAIMED** |
| Security certifications | — | **NOT CLAIMED** |
| Guaranteed production recovery times | — | **NOT CLAIMED** |

Use phrasing such as **“validated in the benchmark harness”** or **“observed in the local runtime”** rather than production guarantees.
