# ESA — Executable State Architecture

Policy-bounded multi-agent runtime for adaptive payment infrastructure.

**Razorpay Buildathon 2026 — Open Track**

> AI proposes typed infrastructure actions from live state; deterministic policy, verification, gateway, and audit execute or block them.

---

## What it does

ESA runs a closed control loop for payment workloads:

1. **Monitor** — detect degradation (latency, queue, errors)
2. **Diagnose** — root cause (Ollama + rule fallback)
3. **Plan** — typed actions (`CREATE_REPLICA`, `SHIFT_ROUTE`, `ROLLBACK`, …)
4. **Safety + Policy** — allow, deny, stale-state reject, approval gates
5. **Gateway** — single execution path with effect measurement
6. **Audit** — replayable decision lineage

Optional **Razorpay Test Mode** webhooks and orders feed real payment events into the same loop.

---

## Architecture

```
Payment events / Razorpay webhooks
           │
           ▼
    State Fabric (versioned workloads)
           │
    ┌──────┴──────┐
    ▼             ▼
 Agents         Telemetry ──► Command Center UI
    │
    ▼
 Policy + Verifier
    │
    ▼
 Action Gateway ──► Runtime mutation + rollback
    │
    ▼
 Audit trail + effect measurement
```

---

## Quick start

### Prerequisites

- Rust 1.70+
- Node 18+ (or Bun) for frontends
- Ollama (optional but recommended for full agent diagnosis)
- `.env` with Razorpay test keys if using live webhooks (see `.env.example` patterns in repo)

### 1. Environment

```bash
cp .env.example .env   # if present, or configure .env manually
ollama pull mistral:latest   # or model in OLLAMA_MODEL
ollama serve
```

### 2. Backend (port 8080)

```bash
cargo run --bin esa-api
```

### 3. Command Center (port 3000)

```bash
cd frontend && npm install && npm run dev
```

### 4. Payment simulator (port 5173)

```bash
cd payment-simulator && npm install && npm run dev
```

### 5. Smoke test

```bash
./scripts/run-demo-test.sh
```

Or use the all-in-one script:

```bash
./scripts/start-demo.sh
```

---

## URLs

| Service | URL |
|---------|-----|
| ESA Command Center | http://localhost:3000 |
| API + health | http://localhost:8080/health |
| Payment simulator | http://localhost:5173 |
| WebSocket telemetry | ws://localhost:3000/ws/telemetry (via Vite proxy) |

---

## Demo API (high level)

| Endpoint | Purpose |
|----------|---------|
| `POST /api/demo/seed` | Seed 3 payment workloads |
| `POST /api/demo/scenario/:name` | `burst-spike`, `stale-state`, `rollback-demo`, … |
| `POST /api/benchmark/harness` | Run B0/B1/B2 benchmark harness |
| `GET /api/audit/trail` | Decision audit records |
| `POST /api/audit/replay/:id` | Deterministic replay |

Full list: start the API and explore routes in `crates/esa-api/src/main.rs`.

---

## Benchmark harness

Three controllers on identical scenarios:

- **B0** — static threshold rules
- **B1** — HPA-style adaptive controller
- **B2** — full ESA (agents + policy + gateway)

```bash
make benchmark-quick      # fast smoke
make benchmark-smoke      # full agent cycle, 1 seed
make benchmark            # 5 seeds × 8 scenarios × 3 controllers
```

Latest summary: [`benchmarkreport.md`](benchmarkreport.md)

---

## Repository layout

```
ESA_paymentgateway/
├── crates/                 # Rust workspace
│   ├── esa-core/           # Types, actions, audit
│   ├── esa-state/          # State fabric + snapshots
│   ├── esa-policy/         # Policy engine
│   ├── esa-agents/         # Monitor, diagnosis, planning, safety
│   ├── esa-gateway/        # Action gateway + rollback
│   ├── esa-runtime/        # Orchestrator
│   ├── esa-api/            # HTTP API + benchmark binary
│   ├── esa-razorpay/       # Razorpay adapter
│   └── esa-telemetry/      # Metrics helpers
├── frontend/               # Command Center (React + Vite)
├── payment-simulator/      # Next.js payment traffic UI
├── benchmarks/             # Harness outputs (raw, processed, scenarios)
├── docs/                   # All project markdown docs
├── scripts/                # Demo, test, seed scripts
├── benchmarkreport.md      # Latest benchmark summary (root)
├── Makefile                # benchmark targets
└── docker-compose.yml      # Optional infra stack
```

---

## Documentation

All guides, PRD, status reports, and verification docs are in [`docs/`](docs/README.md).

| Doc | Use when |
|-----|----------|
| [docs/QUICK_START.md](docs/QUICK_START.md) | Running the 5-minute demo |
| [docs/DEMO_GUIDE.md](docs/DEMO_GUIDE.md) | Pitch script for judges |
| [docs/ESA_paymentprdv2.md](docs/ESA_paymentprdv2.md) | Full PRD |

---

## Development

```bash
# Rust tests
cargo test --all

# Frontend build
cd frontend && npm run build

# Integration smoke
./scripts/integration-test.sh
```

CI: `.github/workflows/ci.yml`

---

## Safety model

- No shell/exec actions from agents — typed `ActionType` enum only
- Every mutation passes **policy** + **state version** checks
- Stale proposals rejected; high-risk actions blocked or require approval
- Rollback via snapshots; audit trail for replay

---

## License

MIT — see [LICENSE](LICENSE) if present.

---

## Team

ESA Team — Razorpay Buildathon 2026
