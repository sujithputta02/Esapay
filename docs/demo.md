# Demo walkthrough

Operator / judge manual for the ESA + Razorpay buildathon demo. All steps use **Test Mode** and the **local benchmark runtime** — not production payment settlement.

## Prerequisites

- `cargo run --bin esa-api` (port 8080)
- Command Center: `cd frontend && npm run dev` (port 3000)
- Payment simulator: `cd payment-simulator && npm run dev` (port 5173)
- Ollama with model from `OLLAMA_MODEL` (recommended for live diagnosis)
- Optional: Kind cluster + `esa-workloads` namespace for live `kubectl scale`

## 1. Start services

```bash
ollama serve
cargo run --bin esa-api
```

Or `./scripts/start-demo.sh` / `docker compose up` (see [reproducibility.md](reproducibility.md)).

## 2. Open payment simulator

http://localhost:5173 — Razorpay Checkout Test Mode (cards shown in UI).

## 3. Seed workloads

```bash
curl -X POST http://localhost:8080/api/demo/seed
```

Or Command Center dashboard after API is up.

## 4. Create test payment

Use simulator checkout with Razorpay test card (e.g. Mastercard `5267 3181 8797 5449`). Payment events feed workload metrics via webhook/confirm paths when keys are configured.

## 5. Trigger spike

```bash
curl -X POST http://localhost:8080/api/demo/scenario/burst-spike \
  -H 'Content-Type: application/json' -d '{"intensity": 1.0}'
```

Watch Command Center: workloads → `Degraded`, vitals spike.

## 6. Observe detection

Orchestrator runs every **5 seconds**. Monitor agent flags conditions; WebSocket `/ws/telemetry` streams `ConditionDetected` events.

## 7. Observe agents

**Agents** page or telemetry: Diagnosis (Ollama or rule fallback) → Planning → Safety advisory.

## 8. Trigger stale state

```bash
curl -X POST http://localhost:8080/api/demo/scenario/stale-state
```

Next proposal with old `state_version` → `StaleState` verdict in audit / verdicts.

## 9. Observe rejection

**Policy** or **Audit** view: `RULE_003_STALE_STATE` / `StaleState` — **no mutation**.

## 10. Observe replan

After fabric version advances, a new orchestrator cycle plans with current version; gateway may `ALLOW` valid `CREATE_REPLICA`.

## 11. Observe Kubernetes mutation (optional)

When cluster + `KUBERNETES_ENABLED` allow, gateway runs `kubectl scale` for mapped deployments. In-memory state updates even if kubectl fails.

## 12. Inject execution failure / rollback

```bash
curl -X POST http://localhost:8080/api/demo/scenario/rollback-demo
```

Demonstrates snapshot + `ROLLBACK` action through gateway.

## 13. Open audit

```bash
curl http://localhost:8080/api/audit/trail
curl http://localhost:8080/api/audit/verify-chain
```

## 14. Run replay

```bash
curl http://localhost:8080/api/audit/replay/<decision_id>
```

Deterministic replay from stored JSON — no LLM re-invocation.

## Scripted narrative

- `./scripts/demo.sh` — 8-step K8s-aware terminal demo
- `FINAL_5MIN_DEMO_SCRIPT.md` — video pitch script
- `FINAL_5MIN_DEMO_SCRIPT.md` — video pitch script

## What we do not demonstrate

- Real money settlement or GMV protection
- RBI / PCI compliance
- Production SLA guarantees

See [claims.md](claims.md).
