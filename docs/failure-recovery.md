# Failure recovery

| Failure | Detection | System response | Mutation? | Recovery |
|---------|-----------|-----------------|-----------|----------|
| LLM timeout / unavailable | Ollama client error | Rule-based diagnosis | No (until gateway success) | Continue cycle with rules |
| Malformed LLM JSON | Parse error in diagnosis | Rule fallback | No | Same |
| Safety veto | `SafetyReview.passed = false` | Skip gateway | **No** | Next cycle |
| Policy violation | `PolicyVerdict::Denied` | Gateway short-circuit | **No** | Audit record |
| Stale state | `StaleState` verdict | Reject proposal | **No** | Replan on next cycle with new version |
| Requires approval | High risk / low confidence | Block auto-exec | **No** | Manual approval (future) |
| Execution apply failure | Gateway apply error | Attempt rollback from snapshot | Partial | `ROLLBACK` action / snapshot restore |
| Max replicas | RULE_001 | Deny CREATE_REPLICA | **No** | SHIFT_ROUTE or alternate action |
| Invalid / critical restart | Policy + gateway | Deny (BENCH-14) | **No** | — |
| Effect underperformed | `EffectMeasurement` status | Recorded in audit | Yes (already applied) | **No auto-replan** in orchestrator today |

## Rollback

- Pre-execution snapshot before mutation
- `ActionType::Rollback` with numeric `target_snapshot` version
- `restore_snapshot` reloads workload map

## LLM failure (BENCH-12 / BENCH-13)

Harness verifies recovery still completes via gateway with **0 unsafe mutations**.

## Kubernetes downstream failure

If `kubectl scale` fails, in-memory state may still update; behavior depends on gateway error handling path.

See [effect-verification.md](effect-verification.md).
