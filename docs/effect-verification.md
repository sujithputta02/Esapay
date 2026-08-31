# Effect verification

After a successful gateway mutation, ESA compares **expected** vs **observed** metric deltas.

## Flow

```text
ExpectedEffect (on proposal)
       ↓
Runtime mutation (metrics before/after)
       ↓
ObservedEffect computed
       ↓
EffectMeasurement::calculate()
       ↓
Status: ObjectiveMet | PartiallyMet | Underperformed | Failed
       ↓
Stored on execution + audit (no automatic replan in orchestrator)
```

## Thresholds (`esa-core`)

| Effectiveness | Status |
|---------------|--------|
| ≥ 0.95 | ObjectiveMet |
| ≥ 0.75 | PartiallyMet |
| ≥ 0.50 | Underperformed |
| &lt; 0.50 | Failed |

## API

- `GET /api/effects/measurements`
- `GET /api/effects/recent`

Data sourced from in-memory `AuditStore` execution records.

## Differentiation

ESA records whether the mutation **achieved the stated effect**, rather than assuming success on gateway return alone.

**Limitation:** Orchestrator does not currently trigger replan solely because `EffectStatus::Failed`.

## Benchmark

- BENCH-10 (`weak_effect`) exercises effectiveness scoring in harness.
