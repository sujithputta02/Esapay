# ESAPay Ethical AI & Security Gap Analysis

**Audit Date:** 2026-11-12  
**Branch:** `main`  
**Reference Frameworks:**
- NIST AI Risk Management Framework (AI RMF 1.0: Govern, Map, Measure, Manage)
- NIST Generative AI Profile (NIST AI 600-1)
- OWASP Top 10 for LLM Applications (LLM01–LLM10)

---

## 1. Re-Verification of Historical Security Findings on `main`

Prior audit documents (`docs/production-readiness/BASELINE_AUDIT.md`, `THREAT_MODEL.md`, `SECURITY_GAP_REPORT.md`) were written against earlier branches/commits. Every historical finding was re-checked directly against current `main`:

| Historical Finding | Prior Status | Current Status on `main` | Verification Evidence in `main` |
|---|---|---|---|
| **JWT Signature Validation Bypass** (`parse_supabase_jwt`) | P0 Open | **FIXED** | `crates/esa-api/src/api_keys.rs:654-839` verifies `HS256`/`RS256`/`ES256` signatures, `exp`, `nbf`, `aud`, `iss`, and `sub` via `jsonwebtoken` + `JwksKeyCache`. |
| **Prefix-Based API Key Bypass** (`is_key_authorized`) | P0 Open | **FIXED** | `api_keys.rs:912-1089` uses constant-time SHA-256 hash comparison against env keys and `SandboxKeyStore` (with revocation & 24h expiry). |
| **Unauthenticated Mutating Routes & Optional Checkout Auth** | P0 Open | **FIXED** | `universal_checkout`, `toggle_gateway`, `create_workload`, `razorpay_create_order`, `trigger_spike`, `seed_demo_data`, `trigger_scenario`, `run_benchmark*`, `replay_decision` all call `authenticate_caller` + scope/admin checks. |
| **`KUBERNETES_ENABLED` Defaulted to `true`** | P0 Open | **FIXED** | `crates/esa-gateway/src/executor.rs:410-414` uses `.unwrap_or(false)`, verified by `test_kubernetes_enabled_default_is_false`. |
| **Razorpay `verify_signature_any` Secret Fallback** | P0 Open | **FIXED** | Removed in `crates/esa-razorpay/src/webhook.rs`; requires dedicated `RAZORPAY_WEBHOOK_SECRET`. |
| **Permissive CORS (`CorsLayer::permissive()`)** | P0 Open | **FIXED** | `crates/esa-api/src/main.rs:399-533` enforces `CorsConfig` origin allowlist (HTTPS required in prod, rejects `*` and loopback in prod). |
| **Frontend Live Secret & `?key=` Query Param** | P0 Open | **FIXED** | `frontend/src/lib/api.ts:29-44` & `frontend/.env.example` only reference `VITE_ESA_TEST_API_KEY`; `?key=` removed. |
| **Razorpay Unknown Status Mapped to Captured** | P0 Open | **FIXED** | `crates/esa-razorpay/src/webhook.rs:562-570` rejects unknown statuses with `WebhookError::UnsupportedStatus`. |
| **Unauthenticated `/ws/telemetry` Stream** | P0 Open | **FIXED** | `crates/esa-api/src/main.rs:4041-4096` & `websocket.rs:90-176` require admin/`telemetry:admin` auth in production; rejects query-param tokens in production. |
| **LLM Concurrency Flooding & Missing Circuit Breaker** | P1 Open | **FIXED** | `crates/esa-agents/src/ollama.rs:39-43, 93-109, 235-244` enforces `Semaphore(4)` and 30s circuit breaker after 5 failures. |
| **In-Memory `StateFabric` & `AuditStore` (No Persistence)** | P1 Open | **STILL OPEN** | `WebhookStore` and `SandboxKeyStore` are now durable in PostgreSQL, but `StateFabric` and `AuditStore` in `main.rs:596-597` remain in-memory (`crates/esa-state/src/store.rs` is unwired). |
| **Process-Local Rate Limiting (`REDIS_URL` Unwired)** | High Open | **STILL OPEN** | `crates/esa-api/src/rate_limiter.rs` uses `BoundedKeyedStateStore` in memory; `REDIS_URL` is unused in `crates/`. |
| **Strict LLM Output Schema (`deny_unknown_fields`)** | Medium Open | **STILL OPEN** | `crates/esa-agents/src/diagnosis.rs:261` `Diagnosis` lacks `#[serde(deny_unknown_fields)]` and confidence range validation (`0.0..=1.0`). |

---

## 2. AI Security & Ethics Gap Analysis (NIST AI RMF + OWASP LLM Top 10)

| # | Risk Area | Framework Mapping | Current State in `main` | Gap & Severity | Required Mitigation |
|---|---|---|---|---|---|
| 1 | **Prompt Injection** | OWASP LLM01 / NIST Map & Manage | `DiagnosisAgent` serializes `Vec<Condition>` into `prompt` without delimiter isolation or input sanitization. Currently low-exposure because `Condition` comes from `StateFabric` metrics, **but** any future inclusion of webhook `notes`, `error_message`, or customer strings creates direct injection risk. | **MEDIUM** (High once payment metadata is added) | Implement `PromptSanitizer` with strict structural separation (system/user roles), character/length bounds, and untrusted-text escaping; never place raw PSP/user strings in system instructions. |
| 2 | **Cross-Merchant Information Leakage** | OWASP LLM02 / NIST Govern & Map | `StateFabric`, `AuditStore`, `AICostTracker`, `/api/agents/activity`, and `OllamaClient::response_cache` are **global, single-tenant** structures. `/api/agents/activity` returns raw `prompt` and `response` strings to any caller with `audit:read`. | **HIGH** | Scope all AI memory, prompt caches, cost logs, and `/api/agents/activity` records by `merchant_id` (or enforce strict `telemetry:admin`-only access for global infra telemetry with zero tenant data). |
| 3 | **Insecure Agent Tool Permissions** | OWASP LLM06 / NIST Govern | Agents can only emit typed `ActionType` variants via `ActionProposal`, gated by `SafetyAgent`, `PolicyEngine`, `DecisionVerifier`, and `ActionGateway`. `KUBERNETES_ENABLED` defaults to `false`. | **LOW** (Strong foundation; maintain invariant) | Keep `ActionType` strictly bounded; never give LLMs raw SQL, HTTP, payment-execution, or shell tools; add `allowlist` validation on `kubectl` deployment names. |
| 4 | **Model Hallucinations** | OWASP LLM09 / NIST Measure | `DiagnosisAgent` uses `extract_json_object` + `normalize_root_cause_str`, which silently coerces unrecognized root causes to `OTHER` and accepts arbitrary `confidence` floats (including `<0` or `>1` or `NaN`). `SafetyAgent` hardcodes 2 of 4 checks to `true`. | **MEDIUM** | Add `#[serde(deny_unknown_fields)]`, finite `0.0..=1.0` confidence clamping, strict `recommended_action` enum validation, and grounded evidence-ref verification against actual `Condition` metrics. |
| 5 | **Unverified Recommendations** | NIST Measure & Manage | `PlanningAgent` trusts `diagnosis.recommended_action` and applies hardcoded `ExpectedEffect` deltas. `DecisionVerifier::verify_resource_limits` is a placeholder (`Ok(())`). `SafetyTestRunner` is a mock returning `Pass`. | **HIGH** | Replace `SafetyTestRunner` mock with real `PolicyEngine`/`ActionGateway` assertions; implement real resource/quota checks in `DecisionVerifier`; require Digital Twin or deterministic corroboration before high-impact shifts. |
| 6 | **Sensitive Data Retention** | OWASP LLM02 / NIST Govern & Map | `AICostTracker::record_inference` (`crates/esa-agents/src/ollama.rs:488-489`) stores up to 2,000 chars of **raw `prompt` and `response`** in memory and serves them via `/api/agents/activity`. No redaction filter exists before logging or caching. | **HIGH** | Run `PromptSanitizer` / PII-secret scrubber *before* inference, caching, and `AICostTracker` recording; store prompt hashes or redacted summaries instead of raw sensitive payloads. |
| 7 | **Untrusted Provider & Webhook Metadata** | OWASP LLM01 / NIST Map | PSP webhooks (`raw_payload`, `error_message`, `notes`) are stored in `webhook_inbox` and `PaymentStatusResult`. Currently **not** fed to LLMs, but future Payment-Domain Intelligence will inspect PSP error reasons. | **MEDIUM** | Treat all PSP metadata as untrusted data; extract only allowlisted enums/error-code tokens before passing to `IntelligenceCore`. |
| 8 | **Model-Generated Actions** | OWASP LLM06 / NIST Manage | `ActionGateway` requires `PolicyEngine` + `DecisionVerifier` + OCC `state_version` match. However, `AuditRecord.agent_reasoning` is left empty by `ActionGateway`, breaking full AI-to-execution lineage. | **MEDIUM** | Populate `AgentReasoningTrace` (provider, model, prompt hash, redacted output, confidence) on every `AuditRecord` and persist `AuditStore` to PostgreSQL. |
| 9 | **Uncontrolled Memory Updates** | OWASP LLM01/LLM08 / NIST Manage | No persistent memory exists yet. If added naively, hallucinated or injected outputs could poison future routing/diagnosis decisions. | **HIGH (Pre-implementation)** | Gate all Episodic/Semantic/Procedural memory writes behind deterministic schema validation, provenance tagging, and human/admin approval for procedural rules. |
| 10 | **Bias & Unfair Merchant/Customer Treatment** | NIST AI RMF Fairness / AI 600-1 | Currently no AI touches per-customer or per-merchant payment routing. However, future AI routing advisors could starve smaller merchants, penalize specific payment methods (e.g., UPI vs Card), or over-throttle regions. | **MEDIUM (Pre-implementation)** | Prohibit demographic/PII features in prompts; enforce deterministic minimum-SLA and fairness bounds in `PolicyEngine`; track per-corridor and per-merchant routing parity metrics. |
| 11 | **Model Availability & Dependency Risks** | OWASP LLM10 / NIST Manage | `DiagnosisAgent` calls Tier-2 LLM **synchronously** in `run_cycle()`. A 25s × 2 timeout on Ollama or a queued/quota-exhausted Hugging Face ZeroGPU Space blocks the 5s orchestration loop. `HF_AI_BRAIN_TOKEN` does not exist yet. | **HIGH** | Implement `ProviderRouter` with strict queue/quota timeouts, non-blocking async Tier-2 fanout (restoring true Dual-Cadence), automatic fallback to Ollama/Tier-3 rules, and backend-only `HF_AI_BRAIN_TOKEN`. |
| 12 | **Benchmark & UI Integrity (Ethics/Transparency)** | NIST Measure / Transparency | `BenchmarksView.tsx` shows hardcoded metrics; `runner.py` prints static ablations; `/api/agents/status` emits hardcoded transcripts; `/api/intent/*` returns static JSON. | **HIGH** | Wire all UI views and API endpoints strictly to live measured data; label simulated vs live metrics explicitly. |

---

## 3. Additional Payment-Path Findings to Resolve Alongside Phase 1

1. **`razorpay_confirm_payment` Authorized State Bug (`crates/esa-api/src/main.rs:2249-2295`):** If Razorpay returns status `"authorized"` (not `"captured"`), `payment_entity_to_event` emits `PaymentAuthorized`, `WebhookStore` transitions `payment_records` to `Authorized`, yet the handler unconditionally marks `payment_attempts.state = "captured"` and returns `"payment_state": "captured"`.
2. **Missing Razorpay Refund HTTP Endpoint:** `RazorpayAdapter::create_refund` exists, but `esa-api` only exposes `/api/cashfree/refunds` and `/api/payu/refunds`.
3. **PayU Webhook Anti-Replay Window:** `crates/esa-payu/src/webhook.rs` verifies the SHA-512 reverse hash and deduplicates `provider_event_id`, but lacks a timestamp freshness check (unlike Cashfree's `x-webhook-timestamp`).

