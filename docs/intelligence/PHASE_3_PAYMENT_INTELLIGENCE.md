# ESAPay Phase 3 — Payment-Domain Intelligence & Persistent Memory

## Overview

Phase 3 evolves ESAPay's Intelligence Core from infrastructure-level diagnostics (`DiagnosisAgent`) into a typed, evidence-grounded, tenant-isolated **Payment Intelligence** subsystem without duplicating payment storage, provider adapters, or model clients.

## Architecture

### 1. Canonical Payment Intelligence Context (`crates/esa-agents/src/payment_context.rs`)
- Read-only context builder `build_payment_intelligence_context(...)` backed by authoritative records from `WebhookStore`:
  - `PaymentAttemptRecord`
  - `PaymentEntity` (from `payment_records`)
  - `WebhookInboxRecord` (from `webhook_inbox`)
  - `RefundRecord` (from `payment_refunds`)
  - `ProviderHealth` (from `RazorpayAdapter`, `CashfreeProvider`, `PayUProvider`)
- Enforces `ExecutionScope::MerchantScoped` ownership before context assembly (`Forbidden` on cross-merchant access).
- Preserves chronological event ordering, detects duplicate or delayed webhooks (`>300s`), flags state discrepancies, records explicit `missing_data` items, and computes `remaining_refundable_minor` deterministically in Rust (never delegated to an LLM).

### 2. Payment Intelligence Agent (`crates/esa-agents/src/payment_intelligence.rs`)
- Reuses `InferenceProvider` (`OllamaProvider` or `HfZeroGpuProvider`), `privacy::sanitize_prompt`, `privacy::redact_sensitive_text`, and `privacy::strip_chain_of_thought`.
- Strict output validation (`PaymentOutputValidator`) with `#[serde(deny_unknown_fields)]`:
  - Rejects unknown JSON fields, non-finite/out-of-bounds confidence, unsupported evidence references not present in `allowed_evidence_refs` or retrieved `memory:<id>` refs, contradictory payment states, and mutating commands.
  - Abstains with `PaymentIncidentRootCause::InsufficientEvidence` when evidence or confidence is insufficient.
  - Falls back to `deterministic_payment_diagnosis(...)` on provider timeout, HTTP 429 ZeroGPU quota exhaustion, or validation failure.

### 3. Read-Only Merchant-Authorized API & Dashboard
- `GET /api/v1/intelligence/payments/summary`
- `POST /api/v1/intelligence/payments/investigate`
- `GET /api/v1/intelligence/payments/providers/health`
- `GET /api/v1/intelligence/memory/recent`
- Frontend view: `frontend/src/pages/IntelligenceView.tsx` (`/intelligence`).

