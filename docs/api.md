# ESA API

HTTP API served by `esa-api` (default `http://localhost:8080`). WebSocket telemetry: `GET /ws/telemetry`.

## Health

### `GET /health`

**Purpose:** Liveness check.

**Response:** `200` with service status JSON.

---

## Workloads

### `GET /api/workloads`

**Purpose:** List all workloads in `StateFabric`.

### `GET /api/workloads/:id`

**Purpose:** Single workload by ID.

### `POST /api/workloads`

**Purpose:** Create workload (JSON body per `Workload` schema).

---

## Payment & Razorpay

### `POST /api/events/payment`

**Purpose:** Ingest synthetic payment event → workload metrics update.

### `POST /api/razorpay/webhook`

**Purpose:** Razorpay webhook receiver (signature verified when secret configured).

### `GET /api/razorpay/status`

**Purpose:** Whether Razorpay adapter is configured and active.

### `POST /api/razorpay/orders`

**Purpose:** Create Test Mode order for checkout.

### `POST /api/razorpay/verify`

**Purpose:** Validate API key pair against Razorpay.

### `POST /api/razorpay/confirm`

**Purpose:** Confirm payment after checkout success.

---

## Vitals & telemetry

### `GET /api/vitals/history`

**Purpose:** Recent aggregated vitals snapshots for Command Center charts.

### `GET /ws/telemetry`

**Purpose:** WebSocket stream (agent activity, conditions, actions, vitals).

---

## Demo scenarios

### `POST /api/demo/seed`

**Purpose:** Seed three payment workloads with baseline metrics.

### `POST /api/demo/trigger-spike`

**Purpose:** Legacy spike trigger (similar to burst).

### `POST /api/demo/scenario/:scenario`

**Purpose:** Named scenario injection.

**Scenarios:** `healthy-baseline`, `burst-spike`, `stale-state`, `constraint-violation`, `regional-skew`, `rollback-demo`

**Request (optional):**

```json
{ "intensity": 1.0 }
```

**Example:**

```bash
curl -X POST http://localhost:8080/api/demo/scenario/burst-spike \
  -H 'Content-Type: application/json' \
  -d '{"intensity": 1.0}'
```

---

## Agents

### `GET /api/agents/status`

**Purpose:** Current agent task labels from orchestrator cycle.

### `GET /api/agents/activity`

**Purpose:** Recent agent activity feed.

---

## Actions & verdicts

### `GET /api/actions/recent`

**Purpose:** Recent executed / proposed actions.

### `GET /api/verdicts/recent`

**Purpose:** Recent policy verdicts.

### `GET /api/verdicts/stats`

**Purpose:** Aggregated verdict counts.

---

## Audit & replay

### `GET /api/audit/trail`

**Purpose:** Last 50 audit records.

### `GET /api/audit/verify-chain`

**Purpose:** SHA-256 chain integrity check.

**Example:**

```bash
curl http://localhost:8080/api/audit/verify-chain
```

### `GET /api/audit/decision/:decision_id`

**Purpose:** Single decision detail.

### `GET /api/audit/replay/:decision_id`

**Purpose:** Deterministic replay metadata (no LLM).

### `POST /api/audit/replay/:decision_id`

**Purpose:** Trigger replay for decision ID.

---

## Effects

### `GET /api/effects/measurements`

**Purpose:** All effect measurements from audit executions.

### `GET /api/effects/recent`

**Purpose:** Recent effect measurement summaries.

---

## Intent

### `GET /api/intent/active`

**Purpose:** Active intent constraints.

### `GET /api/intent/violations`

**Purpose:** Constraint violation records.

---

## Costs

### `GET /api/costs/ai`

**Purpose:** Token / AI cost aggregates (when tracked).

### `GET /api/costs/per-agent`

**Purpose:** Per-agent cost breakdown.

---

## Benchmarks

### `POST /api/benchmark/run`

**Purpose:** Run legacy inline benchmark controller comparison.

### `POST /api/benchmark/harness`

**Purpose:** Run full harness subset via API (same logic as `esa-benchmark`).

### `GET /api/benchmark/latest`

**Purpose:** Last harness result cached in API process.

### `POST /api/benchmark/ablations`

**Purpose:** Run ablation study (3 live variants + 4 modeled offsets).

### `GET /api/benchmark/ablations`

**Purpose:** Return cached ablation result or run fresh.

---

## Metrics

### `GET /api/metrics/tokens`

**Purpose:** LLM token usage metrics.

**Note:** There is no `GET /metrics` Prometheus scrape endpoint on the API today.

---

## Source

Route registration: `crates/esa-api/src/main.rs`.
