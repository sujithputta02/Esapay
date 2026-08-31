# Planning Agent

## Purpose

Convert diagnosis into a typed `ActionProposal` bound to current state version and intent constraints.

## Inputs

- `Diagnosis`
- Related `Condition` list
- `StateFabric::current_version()`
- `IntentManager` (regions, rollback requirements)

## Outputs

- `ActionProposal` with `ActionType`, evidence refs, embedded `state_version`, `rollback_enabled`

## Responsibilities

- Map diagnosis to `CREATE_REPLICA` or `SHIFT_ROUTE`
- If at max replicas → prefer `SHIFT_ROUTE`
- Create default intent if missing

## Can do

- Propose typed actions with `ExpectedEffect` fields
- Attach planning agent ID and evidence strings

## Cannot do

- Execute via gateway (orchestrator calls gateway separately)
- Propose undefined action types
- Skip `state_version` embedding

## Failure behavior

- No `recommended_action` on diagnosis → no proposal (skip workload)

## Latency

- Typically **~100–300 ms** (in-process, no LLM in planning path today)

## Example

```text
Diagnosis: CREATE_REPLICA recommended
→ ActionProposal {
    action: CreateReplica { workload_id, target_region, state_version: 42, ... }
  }
```
