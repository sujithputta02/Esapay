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
- **Logical expiry filtering vs. physical database cleanup:**
  - *Logical filtering:* All read paths (`query_memories`, `get_evaluation`) filter out records where `expires_at <= NOW()`, and `verify_and_promote_outcome` explicitly rejects expired evaluations (`InvalidResponse`) before any promotion.
  - *Physical cleanup:* `purge_expired()` physically deletes expired rows from both `esa_memory_records` and `esa_experience_evaluations`. In `esa-api`, a bounded hourly background task runs `purge_expired()` (never on per-investigation requests) and logs any deletion failures via `tracing::warn!`.
- `store_memory` rejects `memory_id` collisions across different scopes or merchants (`Forbidden`).
- `delete_memory(scope, memory_id)` and `delete_evaluations_for_merchant(scope)` enforce tenant ownership and reject cross-tenant or implicit `System`-scope access (`Forbidden`).

## Controlled Experience Learning Loop & Retention
- Every investigation records an internal pending `OutcomeEvaluationRecord` in `esa_experience_evaluations` (`accuracy = pending`, default `expires_at = created_at + 90 days`). Investigation never mutates payment state.
- Evaluation reads (`get_evaluation`) and verification (`verify_and_promote_outcome`) require explicit `ExecutionScope::MerchantScoped` matching `eval.merchant_id`; `ExecutionScope::System` does not implicitly grant cross-merchant evaluation access.
- **Authoritative outcome verification:** `verify_and_promote_outcome(...)` requires a sealed `AuthoritativeOutcomeProof` (`pub(super)` inside `memory`) that can only be minted by the crate-internal `verify_outcome_from_webhook_store(...)` (`pub(crate)`) after re-fetching `payment_attempts`, `payment_records`, `webhook_inbox`, and `payment_refunds` from `WebhookStore`. Only `OutcomeAccuracy::Correct` creates a verified episodic memory; `PartiallyCorrect` and `Incorrect` update the evaluation record without episodic promotion.
- **Diagnosis-specific strictness:** `PAYMENT_FAILED_AT_PROVIDER` requires durable provider-originated failure events; `WEBHOOK_DELAYED_OR_MISSING` requires verified delayed-event evidence (never an empty inbox alone); `PROVIDER_STATE_DISCREPANCY` requires matching conflicting authoritative/attempt refs; `DUPLICATE_WEBHOOK_SUPPRESSED` remains unsupported for promotion without a durable duplicate-delivery log.
- **Promotion workflow status (dormant in live API):** Investigations create `Pending` evaluations in `esa_experience_evaluations`. Automatic promotion to episodic memory is intentionally dormant in the live `esa-api` runtime until an independently verified resolution/reconciliation workflow is wired; no unrestricted verification endpoint is exposed.
- `verify_and_promote_outcome(...)` executes atomically inside a PostgreSQL transaction (`SELECT ... FOR UPDATE`, `INSERT INTO esa_memory_records`, `UPDATE esa_experience_evaluations`) and is idempotent on repeated verification so duplicate episodic memories are never created.
- No online RL or model weight fine-tuning occurs.

