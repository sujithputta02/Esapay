# ESAPay Phase 3 — Memory Architecture

## Memory Tiers (`crates/esa-agents/src/memory/`)

| Tier | Scope | Persistence | Purpose |
| :--- | :--- | :--- | :--- |
| **Working** (`WorkingMemory`) | Request + Merchant | In-memory only, cleared after investigation | Holds active `investigation_id`, `observed_evidence`, and redacted step notes. |
| **Episodic** (`MemoryType::Episodic`) | `MemoryScope::Merchant` | `InMemoryPaymentMemoryStore` / `PostgresPaymentMemoryStore` (`esa_memory_records`) | Stores **only verified** historical merchant incident outcomes (`VerificationStatus::Verified`). |
| **Semantic** (`MemoryType::Semantic`) | `MemoryScope::System` | Durable (`esa_memory_records`), seeded via `seed_system_knowledge()` | Curated gateway & payment-domain facts with explicit `source_refs`. Cannot contain `merchant_id`. |
| **Procedural** (`MemoryType::Procedural`) | `MemoryScope::System` | Durable (`esa_memory_records`), versioned | Read-only investigation workflows. Cannot contain `merchant_id` or mutating steps. |

## Tenant Isolation & Retention Invariants
- `MemoryRecord::sanitize_and_validate()` enforces:
  1. `MemoryScope::System` records (`Semantic`, `Procedural`) **cannot** carry a `merchant_id`.
  2. `MemoryScope::Merchant` (`Episodic`) records **must** carry a valid non-empty `merchant_id`.
  3. Unverified diagnoses (`VerificationStatus::Unverified`) are rejected from persistent episodic memory.
  4. All text fields pass through `privacy::strip_chain_of_thought` and `privacy::redact_sensitive_text`.
- Expired records (`expires_at <= now`) are filtered on query and removed via `purge_expired()`.
- `delete_memory(scope, memory_id)` enforces tenant ownership and returns `Forbidden` on cross-tenant deletion attempts.

## Controlled Experience Learning Loop
- Every investigation records a pending `OutcomeEvaluationRecord` in `esa_experience_evaluations` (`accuracy = pending`).
- Only explicit post-hoc verification via `verify_and_promote_outcome(...)` promotes `Correct` or `PartiallyCorrect` outcomes into `VerificationStatus::Verified` episodic memory.
- No online RL or model weight fine-tuning occurs.

