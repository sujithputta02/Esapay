# Governance

Deterministic layers between agent proposals and runtime mutations.

## Typed Action IR

Proposals use `ActionType` (`crates/esa-core/src/actions.rs`):

```text
ActionProposal
├── action (CREATE_REPLICA | SHIFT_ROUTE | MIGRATE_PARTITION | … | ROLLBACK)
├── proposal_id, agent, evidence_refs
└── embedded fields per action:
    ├── parameters (workload_id, region, …)
    ├── expected_effect (latency_delta, queue_delta, …)
    ├── risk level
    ├── state_version (OCC)
    ├── rollback_enabled
    └── confidence (where applicable)
```

No free-form shell or kubectl actions exist in the enum.

## Action Gateway pipeline

```text
Agent Proposal
      ↓
Policy Engine (RULE_001–004 + intent constraints)
      ↓
Verdict: ALLOWED | DENIED | STALE_STATE | REQUIRES_APPROVAL
      ↓
Decision Verifier (workload exists, version drift ≤ 5)
      ↓
Pre-execution snapshot (rollback path)
      ↓
Commit-time state_version re-check
      ↓
Runtime mutation (StateFabric)
      ↓
Optional kubectl scale
      ↓
Effect measurement + audit append
```

## Policy rules (implemented)

| Rule | Behavior |
|------|----------|
| RULE_001 | Deny `CREATE_REPLICA` at max replicas |
| RULE_002 | `RequiresApproval` for High/Critical risk (Rollback exempt) |
| RULE_003_STALE_STATE | Deny if `state_version != current_version` |
| RULE_004 | Low confidence (&lt; 0.75) → `RequiresApproval` |

Intent constraints: allowed/forbidden regions, rollback requirements.

## Non-bypassability

> **No agent has direct infrastructure execution privileges.**

- Orchestrator calls `ActionGateway::execute_with_verdict` only after Safety pass.
- Demo/benchmark recovery paths also use gateway for B2 typed actions.
- There is no alternate code path that applies replica changes without policy evaluation in the gateway.

## Approval gate

`RequiresApproval` verdicts block automatic execution in the gateway (logged + audited).

See [state-management.md](state-management.md), [audit-replay.md](audit-replay.md).
