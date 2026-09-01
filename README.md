# ESA — Executable State Architecture

**One-line thesis:** AI agents propose typed infrastructure actions from live payment workload state; deterministic policy, OCC, gateway, effect verification, and audit **decide and execute** — validated in the local benchmark harness, not guaranteed in production.

**Razorpay Buildathon 2026 — Open Track**

---

## 2. Demo / Video

| Resource | Link |
|----------|------|
| 🎥 **5-Minute Live Demo Video** | **[Watch on YouTube](https://youtu.be/77qjP2yK7Og)** |
| Live demo script | [`scripts/demo.sh`](scripts/demo.sh) |
| 5-minute pitch script | [`FINAL_5MIN_DEMO_SCRIPT.md`](FINAL_5MIN_DEMO_SCRIPT.md) |
| Operator manual | [`docs/demo.md`](docs/demo.md) |
| Command Center | http://localhost:3000 |
| Payment simulator | http://localhost:5173 |

▶️ **Recorded Demo Video:** **[https://youtu.be/77qjP2yK7Og](https://youtu.be/77qjP2yK7Og)** (5-minute walkthrough of the Dark Premium Payment Simulator, 4-Agent Ollama reasoning loop, live Kubernetes scaling, and deterministic safety verification).

---

## 3. Problem Statement

Payment gateways face burst traffic, regional skew, and queue buildup. Static rules and scrape-based autoscalers detect incidents slowly (15s polling) and apply coarse actions. LLM agents can reason about context but are unsafe if given direct infrastructure control.

ESA targets **governed adaptive recovery**: contextual proposals under a non-bypassable deterministic boundary.

---

## 4. What ESA Does

1. Ingests payment events (synthetic API, Razorpay Test Mode webhooks/orders).
2. Maintains versioned workload state (`StateFabric`).
3. Runs a **4-agent loop** every 5s: Monitor → Diagnosis → Planning → Safety.
4. Submits **typed** `ActionProposal` to Policy + Gateway (never direct kubectl from agents).
5. Measures effects, appends SHA-256-chained audit records, supports deterministic replay.
6. Optionally scales Kubernetes deployments when cluster + env allow.

---

## 5. Why Existing Approaches Are Insufficient

| Approach | Limitation in evaluated scenarios |
|----------|-----------------------------------|
| Static rules (B0) | 15s detection window; P95 ~236ms tail; ~16.5s above SLA |
| Adaptive HPA-style (B1) | Still scrape-bound; P95 ~257ms; no governance layer |
| Ungoverned LLM ops | No OCC, policy, or typed IR — unsafe mutations risk |

ESA trades ~1.8s deliberation for **250ms detection**, **156ms P95**, and **4.1s time above SLA** in the harness (vs B0/B1 baselines).

---

## 6. Core Innovation

> **Agents propose; deterministic infrastructure decides and executes.**

- Typed Action IR (no shell/exec)
- Optimistic concurrency (stale proposals rejected)
- Single Action Gateway execution path
- Post-execution effect verification
- Snapshot rollback + audit replay without LLM

---

## 7. Architecture Overview

```text
Payment events / Razorpay webhooks
           │
           ▼
    Payment adapter
           │
           ▼
    State Fabric (versioned)
           │
    ┌──────┴──────┐
    ▼             ▼
 Agents         Telemetry (WebSocket)
    │
    ▼
 Policy Engine → Decision Verifier
    │
    ▼
 Action Gateway → Runtime mutation (+ optional kubectl)
    │
    ▼
 Effect measurement + Audit (SHA-256 chain)
```

Detail: [`docs/architecture.md`](docs/architecture.md)

---

## 8. End-to-End Execution Flow

1. Metrics update on payment event or demo scenario.
2. Monitor extracts conditions (latency, queue, errors).
3. Diagnosis hypothesizes root cause (Ollama + rule fallback).
4. Planning synthesizes typed action with `state_version` and `expected_effect`.
5. Safety advisory review (does not execute).
6. Policy engine: ALLOW / DENY / STALE / REQUIRES_APPROVAL.
7. Gateway: commit-time OCC, apply mutation, measure effect, audit.

Detail: [`docs/execution-flow.md`](docs/execution-flow.md)

---

## 9. 4-Agent Architecture

| Agent | Role | Executes? |
|-------|------|-----------|
| Monitor | Condition detection | No |
| Diagnosis | Root-cause hypothesis | No |
| Planning | Typed action proposal | No |
| Safety | Risk advisory | No |

Per-agent docs: [`docs/agents/`](docs/agents/)

---

## 10. Deterministic Governance Layer

- **Typed Action IR** — `CREATE_REPLICA`, `SHIFT_ROUTE`, `ROLLBACK`, …
- **Policy engine** — RULE_001–004 + intent constraints
- **Decision verifier** — workload exists, version drift bounds
- **Action Gateway** — sole mutation path

Detail: [`docs/governance.md`](docs/governance.md)

---

## 11. Payment Runtime / Simulator

| Component | Port | Role |
|-----------|------|------|
| `payment-simulator/` | 5173 | Next.js Razorpay Checkout (Test Mode) |
| `esa-api` payment routes | 8080 | Events, orders, webhooks, confirm |
| `esa-razorpay` | — | Adapter, signature verify, dedup |

Test cards shown in simulator UI (e.g. Mastercard `5267 3181 8797 5449`).

---

## 12. Kubernetes Runtime

- Optional Kind cluster, namespace `esa-workloads`
- Gateway may `kubectl scale` mapped deployments when enabled
- Manifests: `k8s/deployments.yaml`
- In-memory state updates even if kubectl unavailable

---

## 13. Safety Model

- Agents cannot invoke shell, kubectl, or arbitrary APIs
- High/Critical risk → `RequiresApproval` (blocks auto-exec)
- Stale `state_version` → reject before mutation
- 650 adversarial harness trials → **0 unsafe mutations** observed
- LLM failure → rule fallback, no direct infra access

Detail: [`docs/claims.md`](docs/claims.md), [`SECURITY.md`](SECURITY.md)

---

## 14. Failure Handling

Covers LLM timeout, policy violation, stale state, execution failure, effect underperformance, rollback.

Detail: [`docs/failure-recovery.md`](docs/failure-recovery.md)

**Limitation:** Orchestrator does not auto-replan on failed effect verification today.

---

## 15. Benchmark Methodology

- **Controllers:** B0 static, B1 adaptive, B2 full ESA
- **Matrix:** 8 perf × 5 seeds × 3 controllers + 7 safety × 5 seeds = **155 trials**
- **Seeds:** 481923–481927
- **Commands:** `make benchmark`, `make benchmark-quick`, `make benchmark-smoke`

Detail: [`benchmarks/methodology.md`](benchmarks/methodology.md)

---

## 16. Benchmark Results

| Metric | B0 | B1 | B2 ESA |
|--------|----|----|--------|
| P95 | 236 ms | 257 ms | **156 ms** |
| Time above SLA | 16.5 s | 14.8 s | **4.1 s** |
| Stabilization | 9.6 s | 7.2 s | **2.3 s** |
| Total recovery | 24.6 s | 22.2 s | 24.3 s |
| Detection latency | 15.0 s | 15.0 s | **250 ms** |
| Action Gateway + OCC | No | No | **Yes** |
| Stale-state rejections | — | — | **5** |
| Adversarial safety suite | Not tested | Not tested | **0 / 650** |

*B0/B1 are performance baselines without a governance layer—the 650-trial adversarial suite (stale OCC, policy blocks, rollback, LLM failure) applies only to B2. Stale-state rejections are an ESA-only metric.*

Full report: [`benchmarkreport.md`](benchmarkreport.md) · Summary: [`docs/benchmark-results.md`](docs/benchmark-results.md)

---

## 17. Ablation Study

| Variant | Evidence type |
|---------|---------------|
| B1_adaptive, ESA_no_agents, Full_ESA | **Live harness trials** |
| ESA_single_agent, no_versioning, no_effect_verification, no_rollback | **Modeled offsets** from Full_ESA |

Detail: [`benchmarks/ablations.md`](benchmarks/ablations.md)

---

## 18. Adversarial Safety Results

650 predefined attempts across stale OCC, max replicas, region policy, critical risk, malformed payloads, rollback, LLM failure — all blocked or safely handled; SHA-256 chain valid.

Source: `benchmarkreport.md` §6, `esa-gateway/tests/safety_stress_suite.rs`

---

## 19. Reproducibility

```bash
git clone <repo>
cp .env.example .env
docker compose up -d          # optional infra
ollama serve && ollama pull mistral:latest
cargo run --bin esa-api
make benchmark-quick
make audit-verify
```

Detail: [`docs/reproducibility.md`](docs/reproducibility.md)

---

## 20. Quick Start

```bash
cp .env.example .env
ollama serve
cargo run --bin esa-api                    # :8080
cd frontend && npm install && npm run dev  # :3000
cd payment-simulator && npm install && npm run dev  # :5173
./scripts/run-demo-test.sh
```

Also: [`docs/reproducibility.md`](docs/reproducibility.md) and [`docs/demo.md`](docs/demo.md)

---

## 21. Demo Walkthrough

14-step operator flow: seed → spike → agents → stale reject → rollback → audit replay.

Detail: [`docs/demo.md`](docs/demo.md)

---

## 22. API / Endpoints

Implemented routes on `esa-api` (8080):

- Workloads, payment, Razorpay, demo scenarios
- Agents, actions, verdicts, effects, costs, intent
- Audit trail, verify-chain, replay
- Benchmark harness + ablations
- WebSocket: `/ws/telemetry`

Full list: [`docs/api.md`](docs/api.md)

---

## 23. Repository Structure

```text
ESA_paymentgateway/
├── crates/
│   ├── esa-core/           # Types, actions, audit, intent
│   ├── esa-state/          # State fabric, snapshots, store (PG not wired)
│   ├── esa-agents/         # Monitor, diagnosis, planning, safety, Ollama
│   ├── esa-policy/         # Policy engine, verifier
│   ├── esa-gateway/        # Action gateway, rollback, optional K8s
│   ├── esa-runtime/        # Orchestrator
│   ├── esa-api/            # HTTP API, benchmark binary
│   ├── esa-razorpay/       # Razorpay Test Mode adapter
│   └── esa-telemetry/      # Metrics helpers
├── frontend/               # Command Center (React + Vite)
├── payment-simulator/      # Razorpay checkout UI
├── benchmarks/
│   ├── scenarios/taxonomy.yaml
│   ├── raw/, processed/, reports/
│   └── *.md
├── docs/                   # Engineering documentation
├── scripts/                # demo.sh, start-demo.sh, tests
├── k8s/                    # Kubernetes manifests
├── tests/                  # Integration tests
├── benchmarkreport.md      # Latest benchmark summary
├── docker-compose.yml
├── Dockerfile
├── Makefile
├── SECURITY.md
├── CONTRIBUTING.md
├── CHANGELOG.md
└── README.md
```

---

## 24. Configuration

| Variable | Purpose |
|----------|---------|
| `OLLAMA_URL`, `OLLAMA_MODEL` | Diagnosis LLM |
| `RAZORPAY_KEY_ID`, `RAZORPAY_KEY_SECRET` | Test Mode API |
| `RAZORPAY_WEBHOOK_SECRET` | Webhook signature |
| `DATABASE_URL`, `REDIS_URL`, `NATS_URL` | Compose template (**not wired to API state**) |
| `KUBERNETES_ENABLED` | Optional kubectl scale |
| `API_PORT` | Default 8080 |

Template: [`.env.example`](.env.example)

---

## 25. Limitations

- In-memory state fabric (PostgreSQL `StateStore` not connected to API)
- Redis / NATS in Compose only — not used by runtime loop
- No Prometheus `/metrics` on API
- No automatic replan on failed effect verification
- Ablation offsets for 4 of 7 variants (not live feature flags)
- Audit trail not persisted across API restarts
- Benchmark harness environment — not production Razorpay traffic

**Not claimed:** production deployment, RBI/PCI compliance, real GMV protection, settlement, security certifications.

See [`docs/claims.md`](docs/claims.md).

---

## 26. Security / Secrets

- Never commit Razorpay live keys or production credentials
- Use `.env` locally; `.env.example` is safe to commit
- Test Mode only (`rzp_test_…`)
- Agents have no direct infrastructure execution privileges

Detail: [`SECURITY.md`](SECURITY.md)

---

## 27. Technology Stack

| Layer | Technology |
|-------|------------|
| Runtime / API | Rust, Axum, Tokio |
| Agents | Rust + Ollama (local LLM) |
| State | In-memory fabric + OCC |
| Policy / Gateway | Rust crates in workspace |
| Command Center | React, TypeScript, Vite, Tailwind |
| Payment UI | Next.js, Razorpay Checkout |
| Optional infra | Docker Compose, Postgres, Redis, NATS, Prometheus, Grafana |
| Optional K8s | Kind, kubectl scale |
| CI | GitHub Actions |

---

## 28. License

MIT License — see [LICENSE](LICENSE).

Copyright (c) 2026 ESA Team.

---

## Documentation index

Full engineering docs: [`docs/README.md`](docs/README.md) · Claims register: [`docs/claims.md`](docs/claims.md)

**ESA Team — Razorpay Buildathon 2026**
