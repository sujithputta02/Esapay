# ESAPay Baseline Audit Report (Phase 0)

**Document Version:** 1.0.0  
**Evaluator:** Senior Staff Security & Payments Reliability Engineer  
**Date:** October 2026  
**Repository:** `sujithputta02/Esapay`  
**Git Branch:** `security/p0-auth-hardening` (base: `main` at `983a70f`)  
**Workspace Path:** `/Users/sujithputta/ESA_paymentgateway`  

---

## 1. Executive Summary & Audit Baseline

This document provides a comprehensive, evidence-based security and reliability audit of the **ESAPay (Executable State Architecture)** codebase. Every finding in this audit has been verified directly against checked-out source code; no assumptions or unverified claims from prior audit notes or documentation were taken as fact.

### 1.1 Local Workspace & Toolchain Baseline

| Component | Verified Specification |
|---|---|
| **Repository Root** | `/Users/sujithputta/ESA_paymentgateway` |
| **Git Commit Base** | `983a70f` (`origin/main`) |
| **Git Working Tree** | Clean (untracked kick-off prompts preserved) |
| **Active Audit Branch** | `security/p0-auth-hardening` |
| **Rust Toolchain** | `rustc 1.96.0`, `cargo 1.96.0` (edition 2021) |
| **Node / Runtime** | Node `v25.9.0`, Bun `1.3.14` |
| **Python** | Python `3.14.7` |
| **Local Services** | Docker daemon inactive; external databases not connected by default |
| **Baseline Test Status** | Rust workspace: 47 passed, 0 failed. Frontend: 8 passed, 0 failed. Clippy: 0 warnings. Rustfmt: clean. |

---

## 2. Complete HTTP Route & Service Inventory

The API server (`crates/esa-api`) mounts routes with dual paths (`/api/...` and `/api/v1/...`). The table below inventories every endpoint, its purpose, the current authentication posture, and the target access policy.

| Route | Method | Current Auth Mechanism | Mutating? | Target Policy | Risk Category |
|---|---|---|---|---|---|
| `/health`, `/healthz` | GET | None (unconditional 200) | No | Public Liveness | Informational |
| `/api/gateways`, `/api/v1/gateways` | GET | None | No | Authenticated Read | Low |
| `/api/gateways/:name/toggle`, `/api/v1/gateways/:name/toggle` | POST | **None** (Anonymous caller can toggle corridor) | **Yes** | Authenticated Admin Write | **P0 Security** |
| `/api/payments/checkout`, `/api/v1/payments/checkout` | POST | **Optional** (Checks token if provided; allows anonymous if omitted) | No (Read/Route) | Authenticated Merchant Read/Write | **P0 Security** |
| `/api/workloads`, `/api/v1/workloads` | GET | None | No | Authenticated Read | Low |
| `/api/workloads`, `/api/v1/workloads` | POST | **None** (Anonymous caller can mutate workloads) | **Yes** | Authenticated Admin Write | **P0 Security** |
| `/api/workloads/:id`, `/api/v1/workloads/:id` | GET | None | No | Authenticated Read | Low |
| `/api/events/payment`, `/api/v1/events/payment` | POST | **None** (Anonymous caller can ingest synthetic events) | **Yes** | Authenticated Ingestion / Sandbox | **P0 Security** |
| `/api/razorpay/webhook`, `/api/v1/razorpay/webhook` | POST | HMAC-SHA256 with fallback to API secret in test mode | **Yes** | Webhook Signature Only | **P0 Security** |
| `/api/razorpay/status`, `/api/v1/razorpay/status` | GET | None | No | Authenticated Read | Low |
| `/api/razorpay/orders`, `/api/v1/razorpay/orders` | POST | **None** (Anonymous caller creates Razorpay orders) | **Yes** | Authenticated Merchant Write | **P0 Security** |
| `/api/razorpay/verify`, `/api/v1/razorpay/verify` | POST | None | No | Authenticated Admin Read | Low |
| `/api/razorpay/confirm`, `/api/v1/razorpay/confirm` | POST | **None** (Anonymous caller confirms payment IDs) | **Yes** | Authenticated Merchant Write | **P0 Security** |
| `/api/vitals/history`, `/api/v1/vitals/history` | GET | None | No | Authenticated Read | Low |
| `/api/demo/trigger-spike`, `/api/v1/demo/trigger-spike` | POST | **None** (Anonymous caller triggers load spike) | **Yes** | Local / Sandbox Only (Disabled in Live) | **P0 Security** |
| `/api/demo/seed`, `/api/v1/demo/seed` | POST | **None** (Anonymous caller resets workload fabric) | **Yes** | Local / Sandbox Only (Disabled in Live) | **P0 Security** |
| `/api/demo/scenario/:scenario`, `/api/v1/demo/scenario/:scenario` | POST | **None** (Anonymous caller triggers chaos scenario) | **Yes** | Local / Sandbox Only (Disabled in Live) | **P0 Security** |
| `/api/benchmark/run`, `/api/v1/benchmark/run` | POST | **None** (Anonymous caller initiates benchmark) | **Yes** | Local / Sandbox Only (Disabled in Live) | **P0 Security** |
| `/api/benchmark/harness`, `/api/v1/benchmark/harness` | POST | **None** (Anonymous caller writes files to host disk) | **Yes** | Local / Sandbox Only (Disabled in Live) | **P0 Security** |
| `/api/benchmark/latest`, `/api/v1/benchmark/latest` | GET | None | No | Authenticated Read | Low |
| `/api/metrics/tokens`, `/api/v1/metrics/tokens` | GET | None | No | Authenticated Read | Low |
| `/api/agents/status`, `/api/v1/agents/status` | GET | None | No | Authenticated Read | Low |
| `/api/agents/activity`, `/api/v1/agents/activity` | GET | None | No | Authenticated Read | Low |
| `/api/agents/fluid-reasoner/status`, `/api/v1/agents/fluid-reasoner/status` | GET | None | No | Authenticated Read | Low |
| `/api/actions/recent`, `/api/v1/actions/recent` | GET | None | No | Authenticated Read | Low |
| `/api/audit/trail`, `/api/v1/audit/trail` | GET | None | No | Authenticated Auditor Read | Low |
| `/api/audit/verify-chain`, `/api/v1/audit/verify-chain` | GET | None | No | Authenticated Auditor Read | Low |
| `/api/audit/decision/:decision_id`, `/api/v1/audit/decision/:decision_id` | GET | None | No | Authenticated Auditor Read | Low |
| `/api/audit/replay/:decision_id`, `/api/v1/audit/replay/:decision_id` | GET, POST | **None** (Anonymous caller initiates decision replay) | **Yes** | Authenticated Auditor Read/Write | **P1 Security** |
| `/api/benchmark/ablations`, `/api/v1/benchmark/ablations` | GET, POST | **None** | **Yes** | Local / Sandbox Only | **P1 Security** |
| `/api/effects/measurements`, `/api/v1/effects/measurements` | GET | None | No | Authenticated Read | Low |
| `/api/effects/recent`, `/api/v1/effects/recent` | GET | None | No | Authenticated Read | Low |
| `/api/costs/ai`, `/api/v1/costs/ai` | GET | None | No | Authenticated Read | Low |
| `/api/costs/per-agent`, `/api/v1/costs/per-agent` | GET | None | No | Authenticated Read | Low |
| `/api/verdicts/recent`, `/api/v1/verdicts/recent` | GET | None | No | Authenticated Read | Low |
| `/api/verdicts/stats`, `/api/v1/verdicts/stats` | GET | None | No | Authenticated Read | Low |
| `/api/intent/active`, `/api/v1/intent/active` | GET | None | No | Authenticated Read | Low |
| `/api/intent/violations`, `/api/v1/intent/violations` | GET | None | No | Authenticated Read | Low |
| `/api/keys/generate-sandbox`, `/api/v1/keys/generate-sandbox` | POST | None (Rate limit not applied in handler) | No | Public Sandbox Evaluator (Rate limited) | **P1 Security** |
| `/api/keys/validate`, `/api/v1/keys/validate` | POST | Structural length/prefix check; unsigned JWT | No | Public / Authenticated Introspection | **P0 Security** |
| `/api/auth/me`, `/api/v1/auth/me` | GET | Extracts Bearer token; accepts unsigned JWTs | No | Authenticated Caller Introspection | **P0 Security** |
| `/ws/telemetry` | GET | **None** (Anonymous WebSocket handshake) | No (Broadcast) | Authenticated Tenant/Auditor Stream | **P0 Security** |

---

## 3. End-to-End Payment & Webhook Data-Flow Trace

### 3.1 Razorpay Webhook Ingress Flow
```text
Provider (Razorpay)
    │  POST /api/v1/razorpay/webhook
    │  Headers: X-Razorpay-Signature: <hex>
    ▼
Axum Middleware (Security headers + Permissive CORS)
    ▼
razorpay_webhook Handler (main.rs:638)
    ▼
Adapter Rate Limiter (adapter.rs:52) [30 rps Token Bucket in-memory]
    ▼
Signature Verification (adapter.rs:59)
    ├── If test_mode: verify_signature_any (accepts webhook secret OR API key secret)
    └── If live_mode: verify_signature (HMAC-SHA256 hex via constant_time_eq)
    ▼
Payload Parser (webhook.rs:78)
    ├── Parses JSON root
    ├── Extracts entity (payment / order / refund)
    ├── Note: Unmapped status falls back to PaymentCaptured in confirm helper!
    └── Normalizes to PaymentEvent { event_id, amount_cents, region, method }
    ▼
In-Memory Deduplication (adapter.rs:77)
    └── Deduper check on event_id (5000-entry ring buffer in memory)
    ▼
Payment Event Application (payment.rs:14)
    ├── Locates matching WorkloadEntity in StateFabric by Region string match
    ├── Calculates load impact based on method and amount
    ├── Updates metrics (rate_per_min, p95_latency_ms, queue_depth, error_rate)
    ├── If latency > 250ms or error > 5%: sets WorkloadState::Degraded
    └── Commits to in-memory StateFabric via upsert_workload
    ▼
Telemetry Broadcast (main.rs:53)
    └── Sends WorkloadUpdate & VitalsUpdate to all connected WebSockets
    ▼
Autonomous Control Loop (orchestrator.rs: run_forever every 5s)
    ├── MonitorAgent detects Degraded workload
    ├── DiagnosisAgent queries Ollama (or rules fallback)
    ├── PlanningAgent generates ActionProposal (e.g. CREATE_REPLICA)
    ├── SafetyAgent / PolicyEngine evaluates RULE_001..004 and State OCC
    ├── ActionGateway::execute_with_verdict commits state
    │   └── IF KUBERNETES_ENABLED is unset (defaults to true!): executes `kubectl scale`!
    └── AuditStore records SHA-256 hash-chain block in memory
```

---

## 4. Prioritized Vulnerability & Reliability Findings

### P0 (Critical Blockers — Must Fix Before Release)

#### 1. JWT Signature Validation Bypass & Forgery
- **Location:** `crates/esa-api/src/api_keys.rs:80-111` (`parse_supabase_jwt`)
- **Evidence:** 
  ```rust
  let claims: SupabaseJwtClaims = serde_json::from_slice(&decoded).ok()?;
  if let Some(exp) = claims.exp {
      if exp < chrono::Utc::now().timestamp() { return None; }
  }
  Some(claims)
  ```
- **Scenario:** The signature component (`parts[2]`) is completely discarded. No HMAC-SHA256 or RSA/ECDSA verification is executed against a Supabase JWT secret or JWKS.
- **Impact:** Complete authentication bypass. Any attacker can generate an arbitrary JWT payload with `"role": "service_role"`, `"sub": "any_uuid"`, sign it with anything (or nothing), and authenticate as a valid merchant or admin.
- **Remediation:** Implement cryptographic signature verification using `jsonwebtoken` or `jwt-simple` with a configured `SUPABASE_JWT_SECRET` (HS256) or JWKS URL (RS256). Validate issuer (`iss`), audience (`aud`), and expiration (`exp`).

#### 2. Structural & Prefix-Based API Key Authorization Bypass
- **Location:** `crates/esa-api/src/api_keys.rs:114-158` (`is_key_authorized`)
- **Evidence:**
  ```rust
  if clean.starts_with("esa_test_demo_") || clean.starts_with("esa_sandbox_") {
      return clean.len() >= 24;
  }
  if clean == "esa_dev_master_key" || clean.starts_with("esa_dev_") {
      return true;
  }
  if clean.starts_with("esa_test_sec_") || clean.starts_with("esa_live_sec_") {
      return clean.len() >= 32;
  }
  ```
- **Scenario:** The validator only inspects string prefixes and lengths, plus hardcoded `"esa_dev_master_key"`. No database or persistent key registry is consulted.
- **Impact:** Anyone can invent a string `esa_live_sec_0000000000000000000000000` and pass authentication as a live production API key.
- **Remediation:** Remove all prefix-length heuristics and hardcoded dev master keys from production authorization. Maintain an authorized key store or cryptographically validated HMAC signature/token mechanism. Check revocation, expiration, and tenant scope.

#### 3. Unauthenticated Mutating Routes & Optional Authorization in Checkout
- **Location:** `crates/esa-api/src/main.rs:1947-1958` and routes lines 362-486
- **Evidence:** In `universal_checkout`, auth is optional:
  ```rust
  if let Some(token) = api_keys::extract_bearer_token(&headers) {
      if !api_keys::is_key_authorized(&token) { return Err(...); }
  }
  ```
  If no token is supplied, it proceeds anonymously. Furthermore, `/api/gateways/:name/toggle`, `/api/workloads` (POST), `/api/razorpay/orders` (POST), `/api/demo/*`, and `/api/benchmark/*` have zero authentication checks.
- **Scenario:** An anonymous caller can toggle payment gateways to degraded, trigger high-CPU benchmarks, reset demo data, or create live Razorpay orders.
- **Impact:** Denial of service, unauthorized resource consumption, financial disruption.
- **Remediation:** Create a strict route authorization middleware. Enforce mandatory authentication on all mutating endpoints. Isolate demo/benchmark endpoints so they are inaccessible in production mode.

#### 4. Unsafe Infrastructure Default: Kubernetes Enabled When Unset
- **Location:** `crates/esa-gateway/src/executor.rs:411-414`
- **Evidence:**
  ```rust
  if std::env::var("KUBERNETES_ENABLED")
      .map(|v| v == "true" || v == "1")
      .unwrap_or(true) // Defaults to true!
  ```
- **Scenario:** In any environment where `KUBERNETES_ENABLED` is not explicitly set to `"false"`, the gateway invokes `kubectl scale deployment <name> --replicas=<n> -n esa-workloads` whenever replica counts change.
- **Impact:** Accidental unintended mutation of live Kubernetes clusters if the host environment has an active kubeconfig.
- **Remediation:** Change default from `unwrap_or(true)` to `unwrap_or(false)`. Update all documentation (`SECURITY.md`, `.env.example`, `README.md`) to reflect that Kubernetes mutation is disabled by default.

#### 5. Unsafe Webhook Secret Fallback in Razorpay Adapter
- **Location:** `crates/esa-razorpay/src/webhook.rs:41-49`, `crates/esa-razorpay/src/adapter.rs:59-68`
- **Evidence:** `verify_signature_any` falls back to `key_secret` if `webhook_secret` fails or is misconfigured.
- **Scenario:** Webhook secret and API key secret are conflated, violating provider security isolation.
- **Impact:** Compromise or leakage of one secret undermines webhook signature guarantees.
- **Remediation:** Remove `verify_signature_any`. Mandate explicit, dedicated `RAZORPAY_WEBHOOK_SECRET` for webhook verification.

#### 6. Permissive CORS Middleware Across Entire API
- **Location:** `crates/esa-api/src/main.rs:538`
- **Evidence:** `.layer(CorsLayer::permissive())`
- **Scenario:** Allows any origin (`*`) with credentials, exposing API endpoints to Cross-Origin Resource Sharing attacks from malicious websites.
- **Impact:** Malicious web pages loaded in a merchant's browser can trigger unauthenticated API actions.
- **Remediation:** Replace permissive CORS with a strict, configurable origin allowlist (e.g. `ALLOWED_ORIGINS`). Reject wildcards when credentials are used.

#### 7. Secret Leakage in Frontend Bundles & Query Parameter Keys
- **Location:** `frontend/.env.example:11-13`, `frontend/src/lib/api.ts:31-35, 48-51`
- **Evidence:** `VITE_ESA_LIVE_API_KEY` is documented in frontend `.env.example` and read by `api.ts`. Also, `getApiKey()` reads API keys from `window.location.search` (`?key=...`) and caches them in `localStorage`.
- **Scenario:** Live production API keys are bundled into public client-side JavaScript assets or leaked via browser history and Referer headers.
- **Impact:** Direct compromise of live production credentials.
- **Remediation:** Remove all live secret environment variables from frontend configuration. Disallow API keys in URL query strings. Require session authentication for web dashboards.

#### 8. Unknown Payment Status Mapped to Captured
- **Location:** `crates/esa-razorpay/src/webhook.rs:161-162`
- **Evidence:**
  ```rust
  let event_type = match status {
      "captured" => PaymentEventType::PaymentCaptured,
      "authorized" => PaymentEventType::PaymentAuthorized,
      "failed" => PaymentEventType::PaymentFailed,
      _ => PaymentEventType::PaymentCaptured, // Fallback maps unknown to captured!
  };
  ```
- **Scenario:** If Razorpay returns a status such as `"created"`, `"refunded"`, or a future provider status, it is treated as a successful payment capture.
- **Impact:** Financial discrepancy, erroneous order fulfillment, corrupted ledger state.
- **Remediation:** Explicitly reject or map unknown statuses to an `Unknown` or `Pending` event type; never assume success.

#### 9. Unauthenticated WebSocket Telemetry Stream
- **Location:** `crates/esa-api/src/main.rs:485`, `crates/esa-api/src/websocket.rs:90-95`
- **Evidence:** `ws_telemetry_handler` performs no handshake authentication, token validation, or tenant filtering.
- **Scenario:** Anyone can establish a WebSocket connection to `/ws/telemetry` and stream internal metrics, workload details, agent activities, and policy decisions.
- **Impact:** Information disclosure and multi-tenant telemetry leakage.
- **Remediation:** Enforce authentication token check during WebSocket upgrade.

---

### P1 (High Priority Architectural & Reliability Items)

#### 10. Pure In-Memory State & Audit Logs (No Persistence Wired to API)
- **Location:** `crates/esa-api/src/main.rs:76-77`, `docs/claims.md:25`
- **Evidence:** `StateFabric::new()` and `AuditStore::new()` are in-memory. The PostgreSQL `StateStore` in `crates/esa-state/src/store.rs` exists but is completely disconnected from `esa-api`.
- **Impact:** All workloads, vitals, rate limits, and audit hash-chains are wiped on server restart.

#### 11. In-Memory Process-Local Rate Limiting (Horizontal Scaling Drift)
- **Location:** `crates/esa-api/src/rate_limiter.rs:13-17`
- **Evidence:** Governor limiters use `NotKeyed` and `InMemoryState`.
- **Impact:** Limits do not synchronize across multiple container replicas. Spammers on one instance are not throttled on another.

#### 12. Unbounded Concurrency on LLM Inference
- **Location:** `crates/esa-agents/src/ollama.rs`
- **Evidence:** No semaphore or concurrency limiter restricts parallel inference calls to Ollama.
- **Impact:** A traffic burst or cascading failure can spawn dozens of parallel Ollama inference requests, exhausting server memory and causing OOM crashes.

---

## 5. Prioritized Remediation Roadmap

1. **Phase 1 (Immediate P0 Auth Hardening)**:
   - Cryptographic JWT verification with proper key/secret resolution, claim checks (`exp`, `iss`, `aud`), and negative test suite.
   - Secure API-key registry & hash-based verification; eliminate prefix/length bypasses.
   - Route-level authorization middleware enforcing authenticated access on all mutating endpoints.
   - Fix `KUBERNETES_ENABLED` default to `false`.
   - Restrict CORS and eliminate frontend live secret patterns.
   - Fix Razorpay unknown status mapping to fail-closed.
2. **Phase 2 (Payment Correctness & Webhooks)**:
   - Remove `verify_signature_any` secret fallback.
   - Durable idempotency tracking for payment events and webhooks.
   - Formal payment state machine with verified transitions.
3. **Phase 3 (Durable Storage)**:
   - Wire PostgreSQL `StateStore` and audit log persistence.
4. **Phase 4 (Agent Safety & Verification)**:
   - Inference concurrency semaphore and circuit breaker.
   - Strict output schema enforcement with `deny_unknown_fields`.

---

*Baseline Audit completed under Phase 0 criteria. No production code modified.*
