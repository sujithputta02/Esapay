# State management

Primary store: **in-memory `StateFabric`** (`crates/esa-state`).

## State versions (OCC)

- Global fabric version increments on each `upsert_workload`.
- Proposals embed `state_version` at planning time.
- Gateway re-checks version at commit (`RULE_003_STALE_STATE`).

## Snapshots

- `create_snapshot()` → versioned `StateSnapshot` map in memory
- `restore_snapshot(version)` used by `ROLLBACK` action
- `rollback-demo` and BENCH-11 harness use numeric snapshot version strings

## Stale proposal example

```text
Planning reads State v481
        ↓
Another mutation commits
        ↓
Fabric becomes v482
        ↓
Agent submits action with state_version=481
        ↓
Policy / Gateway
        ↓
REJECT: StaleState { current: 482, proposed: 481 }
```

Demo: `POST /api/demo/scenario/stale-state`

## Atomic admission

- `execute_atomic_mutation` on fabric for compare-and-set style updates
- Gateway performs commit-time version check before applying metrics/replica changes

## Concurrent agents

- Single orchestrator loop processes workloads sequentially per cycle
- Race scenarios tested via benchmark BENCH-09 and stale-state demo

## PostgreSQL `StateStore`

- Schema and `StateStore` type exist in `crates/esa-state/src/store.rs`
- **Not connected** to `esa-api` today — persistence is future work

## Workload entity

Each workload tracks: metrics, replication policy, region, state (`Healthy`/`Degraded`), per-workload version counter.
