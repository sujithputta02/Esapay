# ESAPay Phase 3 — Financial Evidence & Governance

## Core Financial Safety Rules
1. **Read-Only Financial Boundary:** `PaymentIntelligenceAgent`, `MemoryRetriever`, and all `/api/v1/intelligence/*` endpoints never initiate payments, captures, refunds, routing overrides, or merchant suspensions. Investigations do not mutate payment state; they persist only an internal `esa_experience_evaluations` record (90-day retention) for post-hoc outcome verification.
2. **Authoritative State Precedence:** Durable `PaymentEntity` state (`payment_records`) takes precedence over `PaymentAttemptRecord.state`. An `authorized` payment is never promoted to `captured` by inference.
3. **Deterministic Financial Math:** Refundable balance (`remaining_refundable_minor`) is computed deterministically from captured amount, processed refunds, and pending refund reservations.
4. **Evidence Grounding:** Every `supporting_evidence_refs` item returned by an LLM must exist in `context.allowed_evidence_refs` or retrieved `memory:<id>` identifiers. Unverified claims trigger deterministic fallback.
5. **Prompt Injection Defense:** Provider metadata and operator questions are sanitized via `privacy::sanitize_prompt` and framed as untrusted data. Any model output recommending mutating commands (e.g. `execute_refund`, `drop table`) is rejected by `PaymentOutputValidator`.
6. **Least-Privilege Tenant Scope:** Non-admin callers require `"payments:read"` and are strictly bound to `caller.merchant_id`. Cross-merchant queries return `403 Forbidden` (or `404 NotFound` for unowned transactions).

