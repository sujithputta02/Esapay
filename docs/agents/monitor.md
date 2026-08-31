# Monitor Agent

## Purpose

Read-only observation of workload metrics and state; detect operational conditions.

## Inputs

- All `WorkloadEntity` records from `StateFabric`
- Metrics: P95/P50 latency, queue depth, error rate, rate/min, workload state

## Outputs

- `Vec<Condition>` with type, severity, workload_id, description

## Responsibilities

- Scan workloads each orchestrator cycle
- Emit structured conditions when thresholds exceeded

## Can do

- Detect `HIGH_LATENCY` (P95 > 250 ms)
- Detect `QUEUE_BACKLOG` (queue > 1000)
- Detect `HIGH_ERROR_RATE` (error rate > 0.05)
- Detect `WORKLOAD_DEGRADED` (degraded/overloaded state)

## Cannot do

- Propose or execute actions
- Call external APIs
- Modify workload state

## Failure behavior

- Empty workload list → no conditions (healthy cycle)

## Latency

- Synchronous in-process scan; typically &lt; 20 ms in benchmark model

## Example

```text
Workload payment-upi-india-south: P95=380ms, queue=2500
→ HIGH_LATENCY + QUEUE_BACKLOG + WORKLOAD_DEGRADED
```

**Note:** `NODE_FAILURE` exists in the condition enum but is **not emitted** by current `observe()` implementation.
