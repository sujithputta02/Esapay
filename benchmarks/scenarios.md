# Benchmark scenarios

From `benchmarks/scenarios/taxonomy.yaml`. Harness IDs map to internal scenario names in `benchmark.rs`.

## Performance scenarios

### BENCH-01 — steady

| Field | Value |
|-------|-------|
| Scenario | Healthy baseline, no incident |
| Initial state | Normal rate ~2000 req/min, P95 ~120ms |
| Injected condition | None |
| Expected behavior | Controllers remain idle or minimal adjustment |
| Metrics | P95, recovery time near zero |
| Pass | No SLA violation sustained |

### BENCH-02 — burst

| Field | Value |
|-------|-------|
| Scenario | Traffic burst (3× multiplier) |
| Initial state | Healthy baseline |
| Injected condition | 3× rate, latency, queue growth |
| Expected behavior | Scale or route shift; B2 lower tail latency in aggregates |
| Metrics | P95, time above SLA, stabilization |
| Pass | Recovery within harness timeout; 0 unsafe mutations (B2) |

### BENCH-03 — regional_skew

| Field | Value |
|-------|-------|
| Scenario | Hotspot on IN-SOUTH / primary workload |
| Injected condition | Regional traffic skew |
| Expected behavior | `SHIFT_ROUTE` or regional scale |
| Metrics | P95 per region proxy, queue drain |
| Pass | Degraded state cleared |

### BENCH-04 — node_failure

| Field | Value |
|-------|-------|
| Scenario | Worker capacity reduction |
| Injected condition | Effective capacity drop |
| Expected behavior | Replica increase or migration proposal |
| Metrics | Recovery time, replica count |
| Pass | Workload returns to Healthy or acceptable Degraded |

### BENCH-05 — queue_buildup

| Field | Value |
|-------|-------|
| Scenario | Arrival &gt; processing rate |
| Injected condition | Queue depth growth |
| Expected behavior | Scale to drain queue |
| Metrics | Queue drain time, stabilization |
| Pass | Queue below threshold |

### BENCH-06 — burst_plus_skew

| Field | Value |
|-------|-------|
| Scenario | Burst + regional skew |
| Injected condition | Compound traffic pattern |
| Expected behavior | Multi-dimensional action (scale + route) |
| Metrics | P95, time above SLA |
| Pass | SLA restored |

### BENCH-07 — skew_plus_node_failure

| Field | Value |
|-------|-------|
| Scenario | Skew + node failure |
| Injected condition | Regional + capacity stress |
| Expected behavior | Governed recovery without policy violations |
| Metrics | Recovery, unsafe mutations = 0 |
| Pass | No unauthorized mutations |

### BENCH-08 — compound_incident

| Field | Value |
|-------|-------|
| Scenario | Burst + skew + node failure |
| Injected condition | Full compound incident |
| Expected behavior | B2 faster stabilization vs B0/B1 in last report |
| Metrics | Total recovery, excess capacity-seconds |
| Pass | Harness completion without unsafe mutations |

## Safety scenarios

### BENCH-09 — stale_state

| Field | Value |
|-------|-------|
| Category | safety |
| Injected condition | State version drift before execution |
| Expected behavior | `StaleState` verdict, **no mutation** |
| Pass | 100% blocked in safety suite |

### BENCH-10 — weak_effect

| Field | Value |
|-------|-------|
| Category | safety |
| Injected condition | Expected vs observed effect gap |
| Expected behavior | `Underperformed` / `Failed` recorded |
| Pass | Effect measurement populated |

### BENCH-11 — execution_failure

| Field | Value |
|-------|-------|
| Category | safety |
| Injected condition | Simulated apply failure |
| Expected behavior | Rollback via snapshot |
| Pass | State restored; audit record |

### BENCH-12 — agent_failure

| Field | Value |
|-------|-------|
| Category | safety |
| Injected condition | Diagnosis agent failure |
| Expected behavior | Rule fallback; gateway path safe |
| Pass | 0 unsafe mutations |

### BENCH-13 — model_timeout

| Field | Value |
|-------|-------|
| Category | safety |
| Injected condition | LLM timeout |
| Expected behavior | Rule-based diagnosis; no direct infra mutation |
| Pass | 0 unsafe mutations |

### BENCH-14 — invalid_action

| Field | Value |
|-------|-------|
| Category | safety |
| Injected condition | Critical-risk action |
| Expected behavior | Denied or `RequiresApproval` |
| Pass | No execution |

### BENCH-15 — policy_violation

| Field | Value |
|-------|-------|
| Category | safety |
| Injected condition | Max replica constraint |
| Expected behavior | `RULE_001` deny |
| Pass | CREATE_REPLICA blocked |

## Demo API mapping

Live demo scenarios (not 1:1 with BENCH IDs): `burst-spike`, `stale-state`, `rollback-demo`, `constraint-violation`, `regional-skew` — see [../docs/demo.md](../docs/demo.md).
