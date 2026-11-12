# ESAPay Phase 1 — Payment State Correctness, AI Privacy Hardening & Test Baseline

**Branch:** `fix/phase-1-payment-ai-safety` (branched from `main`)  
**Scope:** Payment-state correctness, AI telemetry privacy & tenant isolation, strict model output validation readiness, and reproducible workspace test baselines. (No Hugging Face ZeroGPU integration, model-provider abstraction, or persistent AI memory added in this phase.)

---

## 1. Razorpay Payment-State Correctness

### Verified Findings
1. **`razorpay_confirm_payment` (`crates/esa-api/src/main.rs`):**
   - Previously used a single `confirm_{payment_id}` inbox key regardless of Razorpay payment status, unconditionally called `payment::apply_payment_event` and `publish_payment_side_effects`, updated `payment_attempts.state = "captured"`, and returned `"success": true` / `"payment_state": "captured"` even when `payment_entity_to_event` produced `PaymentEventType::PaymentAuthorized` or `PaymentEventType::PaymentFailed`.
   - Missing/empty `"currency"` or `"amount"` fell back to `"INR"` / `0`, and unsupported statuses (such as `"created"` or `"pending"`) returned `500 Internal Server Error` via `AppError::Internal`.
2. **`razorpay_webhook` (`crates/esa-api/src/main.rs`):**
   - Previously called `payment::apply_payment_event` whenever `WebhookProcessingResult::Applied` was returned, including `PaymentAuthorized` and `TransitionOutcome::DuplicateIgnored` (e.g., a `payment.captured` webhook arriving after `/api/v1/razorpay/confirm`).
3. **`RazorpayAdapter::get_payment_status` (`crates/esa-razorpay/src/adapter.rs`):**
   - `"created"` and `"pending"` previously fell into `ProviderPaymentStatus::Unknown` instead of `ProviderPaymentStatus::Pending`.
4. **Frontend Checkout (`frontend/src/lib/checkout.ts`):**
   - Non-captured Razorpay confirmation responses (`authorized`, `already_authorized`, `pending`) fell into `'failed'` instead of `'pending'`; never reports `'captured'` without authoritative capture.

### Before vs. After State Transitions

| Razorpay Status | Before (`razorpay_confirm_payment`) | After (`razorpay_confirm_payment`) |
| --- | --- | --- |
| `authorized` | Durable `Authorized`, but `payment_attempts` set to `"captured"`, response `"payment_state": "captured"`, workload side effects triggered | Durable `Authorized`, `payment_attempts = "authorized"`, `confirm_auth_{id}` inbox key, response `success: false, captured: false, payment_state: "authorized"`, **no** captured workload side effects |
| `authorized` → `captured` | Collided on `confirm_{id}` inbox key (`Mismatch` error) | `confirm_auth_{id}` records `Authorized`; subsequent `confirm_{id}` transitions `Authorized -> Captured` and triggers side effects once |
| `failed` | `payment_attempts = "captured"`, response `"payment_state": "captured"`, side effects triggered | Durable `Failed`, `payment_attempts = "failed"`, `confirm_failed_{id}`, response `success: false, captured: false, payment_state: "failed"`, **no** side effects |
| `created` / `pending` | `500 Internal Server Error` | Fails closed (`409 Conflict`), updates `payment_attempts` to `"pending"` if `"created"`, **no** `payment_records` capture, **no** side effects |
| Unknown status | `500 Internal Server Error` | Fails closed (`409 Conflict`), **no** state mutation or side effects |
| Duplicate webhook after confirm | Re-ran `apply_payment_event` on `DuplicateIgnored` | `razorpay_webhook` triggers `apply_payment_event` only on `TransitionOutcome::Applied { new_state: PaymentState::Captured, .. }` |
| Confirm after captured webhook | Returned `already_confirmed` (`payment_attempts` could remain `"created"`) | Syncs `payment_attempts = "captured"` if needed, returns `already_confirmed` (`idempotent: true, workload_updated: false`) |

---

## 2. AI Privacy & Inference Telemetry Hardening

### Changes Implemented
- **Redaction Layer (`crates/esa-agents/src/privacy.rs`):**
  - `redact_sensitive_text` / `sanitize_prompt` scrubs key-value secrets (`api_key`, `webhook_secret`, `client_secret`, `key_secret`, `merchant_salt`, `password`, `cvv`, `cvc`, `card_number`, `pin`, `private_key`, `access_token`, `refresh_token`, `auth_token`), prefixed tokens (`esa_live_*`, `esa_test_*`, `rzp_live_*`, `rzp_test_*`, `sk_live_*`, `sk_test_*`, `hf_*`), `Bearer` tokens, JWTs (`eyJ...`), Luhn-valid PANs (13–19 digits), emails, and `+`-prefixed international phone numbers.
  - `strip_chain_of_thought` removes `<think>...</think>` blocks (including unclosed trailing blocks).
  - `categorize_error` maps raw errors to bounded error categories (`timeout`, `rate_limited`, `circuit_open`, `upstream_http_error`, `parse_error`, `connection_error`, `inference_error`).
- **Telemetry Storage & Retention (`crates/esa-agents/src/ollama.rs`):**
  - `OllamaClient::generate_with_context` sanitizes prompts before caching, sending to Ollama, logging errors, or recording metrics.
  - `InferenceMetrics` no longer stores unrestricted raw `prompt` or `response` strings; it records `merchant_id`, `prompt_hash`, `redacted_prompt_summary`, `redacted_response_summary`, `evidence_refs`, `redaction_applied`, `error_category`, token estimates, and latency.
  - Added `TelemetryRetentionConfig` configurable via `ESA_AI_TELEMETRY_MAX_RECORDS`, `ESA_AI_TELEMETRY_RETAIN_SUMMARY`, `ESA_AI_TELEMETRY_MAX_SUMMARY_CHARS`, and `ESA_AI_TELEMETRY_TTL_SECS`.
- **Endpoint Authorization & Tenant Isolation (`crates/esa-api/src/main.rs`):**
  - `/api/agents/activity` (`get_agent_activity` / `build_agent_activity_for_caller`) enforces `audit:read` scope and filters records to `caller.merchant_id` for non-admin callers (`caller.is_admin()` sees all redacted scopes), returning only redacted summaries and safe diagnostic metadata.

---

## 3. Model Prompt & Response Validation Readiness

- **Provider-Agnostic Validator (`crates/esa-agents/src/validation.rs`):**
  - Added `OutputValidator` and `ValidationLimits` enforcing:
    - Max raw output payload size (`16 KiB` default).
    - Stripping `<think>...</think>` before JSON extraction.
    - `#[serde(deny_unknown_fields)]` on `RawDiagnosisPayload` to reject unexpected/injected fields.
    - Finite `confidence` within `[0.0, 1.0]`.
    - Bounded `hypothesis` (`<= 512` chars), `evidence_refs` (`<= 16` items, `<= 128` chars each), and `recommended_action` (`<= 64` chars).
    - Strict `recommended_action` allowlist (`CREATE_REPLICA`, `SHIFT_ROUTE`, or `None`).
    - Evidence-reference grounding against known telemetry metrics (`p50_latency_ms`, `p95_latency_ms`, `p99_latency_ms`, `rate_per_min`, `queue_depth`, `error_rate`, `workload_state`, `current_replicas`) and observed `Condition` types/workload IDs.
    - Explicit `RootCause::Uncertain` / abstain outcome (`recommended_action: None`) when `root_cause == "UNCERTAIN"`, confidence `< 0.4`, or `evidence_refs` is empty for non-empty conditions.
- **Agent Integration (`crates/esa-agents/src/diagnosis.rs`):**
  - `DiagnosisAgent` wraps untrusted telemetry in `<OBSERVED_TELEMETRY>` with prompt-injection boundary instructions, sanitizes serialized conditions, validates both `FluidReasonerClient` and LLM outputs via `OutputValidator`, and falls back safely to deterministic `rule_based_diagnosis` on malformed/rejected outputs.

---

## 4. Test Baseline & Sandbox Compatibility

- Added explicit `new_without_proxy` / `with_config_no_proxy` test constructors (and `#[cfg(test)]` `.no_proxy()` on `fetch_jwks_endpoint_async` in `crates/esa-api/src/api_keys.rs`) so test clients do not trigger macOS Seatbelt `system-configuration` `SCDynamicStore` panics, while preserving default proxy behavior in production constructors.
- Added:
  - 10 Razorpay payment-state regression tests in `crates/esa-api/src/main.rs`.
  - `/api/v1/agents/activity` redaction & tenant-isolation integration test in `crates/esa-api/src/main.rs`.
  - Redaction, chain-of-thought stripping, retention config, and tenant-scoping unit tests in `crates/esa-agents/src/privacy.rs` and `crates/esa-agents/src/ollama.rs`.
  - `OutputValidator` unit tests in `crates/esa-agents/src/validation.rs`.

---

## 5. Remaining Security Risks & Prerequisites Before Hugging Face ZeroGPU (Phase 2)

1. **In-Memory Telemetry Retention vs. Persistent Store:** `AICostTracker` stores metrics in process memory; multi-instance deployments will need a tenant-scoped durable store or centralized redacted sink if cross-node audit history is required.
2. **Audit/Activity Role Granularity:** Non-admin `SandboxKeyRecord` keys currently lack `audit:read`; ensure Phase 2 RBAC/scope provisioning issues `audit:read` only to authorized merchant audit roles.
3. **ZeroGPU Prerequisites (Next Phase):**
   - Server-side `HF_AI_BRAIN_TOKEN` injection only (never exposed to `frontend/`).
   - Strict provider abstraction (`ModelProvider` trait) routing both Ollama and Hugging Face ZeroGPU responses through `OutputValidator` and `privacy::redact_sensitive_text`.
   - Explicit timeouts, queueing/concurrency caps, and graceful fallback to deterministic rules when ZeroGPU quota is exhausted or cold-starting so payment execution is never blocked.
