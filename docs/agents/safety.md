# Safety Agent

## Purpose

Constitutional pre-gateway review of proposals (advisory gate before policy engine).

## Inputs

- `ActionProposal`

## Outputs

- `SafetyReview`: `passed` + per-check results

## Responsibilities

- Verify rollback enabled on action
- Verify non-empty evidence refs
- Confirm action is within bounded typed contract (no shell/kubectl)

## Can do

- **Veto** proposal before gateway (`passed = false` → orchestrator skips execution)

## Cannot do

- Override policy engine verdict
- Execute mutations
- Modify proposals (policy engine may `MODIFIED` separately — not in safety agent)

## Failure behavior

- Failed checks → orchestrator logs blocked execution; no gateway call

## Latency

- **~50–150 ms** (synchronous checks)

## Example

```text
Proposal missing evidence_refs
→ SafetyReview.passed = false
→ Gateway not invoked
```

**Note:** Final authorization is **Policy Engine + Gateway OCC**, not Safety alone.
