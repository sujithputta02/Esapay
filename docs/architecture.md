# Architecture

ESA separates **proposal** (agents + LLM) from **authorization and execution** (policy, OCC, gateway).

## Layered flow

```text
Payment events / Razorpay webhooks
           │
           ▼
    Payment adapter (esa-razorpay, esa-api payment routes)
           │
           ▼
    State Fabric (versioned workload entities, in-memory)
           │
           ├──────────────────┐
           ▼                  ▼
    Monitor Agent      Telemetry (WebSocket / vitals API)
           │
           ▼
    Diagnosis Agent (Ollama + rule fallback)
           │
           ▼
    Planning Agent (typed ActionProposal)
           │
           ▼
    Safety Agent (constitutional pre-check)
           │
           ▼
    Typed Action IR (ActionType enum)
           │
           ▼
    Policy Engine (RULE_001–004 + intent constraints)
           │
           ▼
    Decision Verifier (workload exists, version drift)
           │
           ▼
    Action Gateway (single execution path)
           │
           ├─► Effect measurement
           ├─► Audit append (SHA-256 chain)
           └─► Optional kubectl scale (KUBERNETES_ENABLED)
```

## Core principle

> **Agents propose; deterministic infrastructure decides and executes.**

No agent holds direct Kubernetes or shell execution privileges. Proposals that fail policy, OCC, or safety checks are rejected before mutation.

## Runtime modes

| Mode | What runs |
|------|-----------|
| **Default demo** | In-memory `StateFabric` + optional kubectl side-effect |
| **Benchmark harness** | Same `StateFabric`; controllers B0/B1/B2 on identical scenarios |
| **Docker Compose** | Postgres/Redis/NATS/Ollama containers defined but **API uses in-memory state** today |

## Crate map

| Crate | Role |
|-------|------|
| `esa-core` | Types, actions, audit, intent |
| `esa-state` | State fabric, snapshots, OCC version |
| `esa-agents` | Monitor, diagnosis, planning, safety |
| `esa-policy` | Policy engine, decision verifier |
| `esa-gateway` | Action gateway, mutations, rollback |
| `esa-runtime` | Orchestrator loop |
| `esa-api` | HTTP API, benchmarks, WebSocket |
| `esa-razorpay` | Test Mode adapter |

See also: [execution-flow.md](execution-flow.md), [governance.md](governance.md), [state-management.md](state-management.md).
