# Execution flow

End-to-end path for autonomous recovery (orchestrator cycle).

## 1. Background loop

`EsaOrchestrator::run_forever` runs every **5 seconds** (`crates/esa-runtime/src/orchestrator.rs`).

## 2. Monitor

- Reads all workloads from `StateFabric`.
- Emits `Condition` list: `HIGH_LATENCY`, `QUEUE_BACKLOG`, `HIGH_ERROR_RATE`, `WORKLOAD_DEGRADED`.
- Thresholds: P95 > 250 ms, queue > 1000, error rate > 0.05, degraded/overloaded state.

If no conditions → cycle ends.

## 3. Per degraded workload

### Diagnosis

- Builds prompt from conditions; calls Ollama (`generate_with_agent("diagnosis", …)`).
- On failure or parse error → **rule-based diagnosis** (no unsafe execution).

### Planning

- Requires `diagnosis.recommended_action`.
- Creates `ActionProposal` with embedded `state_version` from fabric.
- Action types: primarily `CREATE_REPLICA` or `SHIFT_ROUTE`.

### Safety

- Checks: rollback enabled, evidence present, bounded action contract.
- If `!passed` → execution skipped for that proposal.

### Gateway

1. Policy evaluation → verdict
2. Decision verifier
3. Pre-execution snapshot (for rollback path)
4. Commit-time OCC re-check on `state_version`
5. Apply mutation to in-memory workload metrics/replicas
6. Optional `kubectl scale` if Kubernetes integration enabled
7. Effect measurement vs `ExpectedEffect`
8. Audit record appended with hash chain

## 3. Demo API shortcuts

`POST /api/demo/scenario/:scenario` injects incidents without external traffic:

| Scenario | Effect |
|----------|--------|
| `healthy-baseline` | Reset healthy metrics |
| `burst-spike` | Traffic/latency spike, degraded |
| `stale-state` | Bump fabric/workload versions |
| `constraint-violation` | Max replicas + degraded |
| `regional-skew` | Hotspot on first workload |
| `rollback-demo` | Snapshot → mutate → `ROLLBACK` |

## 4. Payment path

1. Payment simulator or Razorpay webhook → API
2. Workload metrics updated in `StateFabric`
3. Same monitor cycle may detect degradation

See [agent-model.md](agent-model.md) and [failure-recovery.md](failure-recovery.md).
