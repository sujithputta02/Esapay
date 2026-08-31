# Audit and replay

## Audit store

- In-memory `AuditStore` (`crates/esa-core/src/audit.rs`)
- **Not persisted** to PostgreSQL in current API

## Hash chaining

Each append:

1. Serializes record payload
2. Sets `previous_hash` from prior record’s `current_hash`
3. Computes SHA-256 `current_hash`

Genesis previous hash: 64 zero hex digits.

## Verification

```bash
GET /api/audit/verify-chain
```

Returns validity + record count from `audit_store.verify_chain()`.

Tamper detection tested in `tamper_detection_test`.

## Decision replay

```bash
GET  /api/audit/replay/:decision_id
POST /api/audit/replay/:decision_id
```

Uses `DecisionReplayer`:

- Reconstructs policy/verification outcome from **stored JSON**
- **No LLM** re-invocation
- `can_replay` when policy result JSON present
- `replayed_verdict` matches stored verdict label (not live re-evaluation against current fabric)

## Other audit endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/audit/trail` | Recent audit records (last 50) |
| GET | `/api/audit/decision/:decision_id` | Single decision detail |

## What replay does **not** do

- Re-run Ollama diagnosis
- Re-execute kubectl or live mutations
- Prove historical cluster state (in-memory model only)

## Trail contents

Action type, policy verdict, expected/observed effects, snapshot version, state versions, timestamps, trace IDs.
