# ESAPay Platform — Security Gap Report & Hardening Plan

**Document Version:** 1.0.0  
**Target Specification:** `SECURITY_HARDENING.md` (79 Security Rules)  
**Evaluator:** Antigravity Autonomous Security Engineer  
**Date:** September 2026  

---

## 1. Executive Summary

This report assesses the current **ESAPay Autonomous Payment Gateway & AI Agent Platform** against the 79-point **AI API & Agent Platform Security Hardening Specification**.

ESAPay exhibits strong architectural security fundamentals:
1. **Deterministic Policy Control**: The LLM is never an authorization authority. Proposals from LLM/agents must pass through the deterministic `SafetyAgent` and `PolicyEngine` before any action is executed.
2. **Multi-Tenant Database with RLS**: Supabase PostgreSQL uses Row-Level Security (RLS) policies where merchants only access their own API keys and data.
3. **Cryptographic Integrity**: API keys are stored as SHA-256 hashes (`key_hash`). Audit logs use cryptographic hash-chaining.
4. **Token-Bucket Rate Limiting**: The Axum backend enforces rate limiting on checkout, API key generation, and simulation triggers via `governor`.
5. **Clean Credential Hygiene**: All production keys and database passwords reside exclusively in environment files (`.env`, `.env.local`) and are git-ignored.

This report identifies remaining gaps, categorizes risks by severity, and lays out the exact remediation plan.

---

## 2. Current Architecture Overview

```text
                               CLIENTS
              (Web Dashboard, Claude Code CLI, Python/TS SDKs)
                                  │
                    HTTPS / Authorization: Bearer
                                  ▼
                   ┌──────────────────────────────┐
                   │       API Gateway (Axum)     │
                   │  - /api/v1/ Versioning       │
                   │  - Token-Bucket Limiter      │
                   │  - API Key & JWT Validator   │
                   │  - Input Bounds Sanitizer    │
                   └──────────────┬───────────────┘
                                  │
         ┌────────────────────────┼────────────────────────┐
         │                        │                        │
         ▼                        ▼                        ▼
 ┌───────────────┐       ┌─────────────────┐      ┌─────────────────┐
 │ StateFabric   │       │ Multi-Gateway   │      │ AI Orchestrator │
 │ (In-Memory)   │       │ Corridor Router │      │  (Autonomous)   │
 └───────┬───────┘       └────────┬────────┘      └────────┬────────┘
         │                        │                        │
         ▼                        ▼                        ▼
 ┌───────────────┐       ┌─────────────────┐      ┌─────────────────┐
 │ Supabase DB   │       │ Payment Corridors│     │ Policy Engine   │
 │ - RLS Enforced│       │ (Razorpay,      │      │ (Deterministic  │
 │ - API Ledger  │       │  PhonePe, etc.) │      │  Safety Check)  │
 └───────────────┘       └─────────────────┘      └────────┬────────┘
                                                           │
                                                  ┌────────┴────────┐
                                                  ▼                 ▼
                                             Ollama 24/7       Action
                                              Inference       Execution
```

---

## 3. Existing Security Controls (Verified)

| Security Domain | Implemented Controls | Verification |
| :--- | :--- | :---: |
| **Secret Protection (Rule 1)** | Zero hardcoded provider or Supabase credentials in frontend/backend bundles. Git-ignored `.env` / `frontend/.env.local`. | Verified with `git grep` (0 leaks) |
| **Model Authorization (Rule 2)** | LLM is restricted to proposing diagnoses. Deterministic `SafetyAgent` checks state version, confidence, and system risk before any execution. | Verified in `esa-runtime` & `esa-policy` |
| **API Key Architecture (Rule 5-6)** | High-entropy random keys (`esa_live_sec_...`). SHA-256 hash stored in DB; plaintext never stored. Instant Sandbox keys supported. | Verified in `api_keys.rs` & `supabase/schema.sql` |
| **Tenant Isolation (Rule 8-9)** | Supabase RLS enforces `merchant_id IN (SELECT id FROM merchants WHERE user_id = auth.uid())`. | Verified in Supabase production SQL |
| **Rate Limiting (Rule 11)** | Governor token-bucket on `/api/v1/payments/checkout` (30 rps, 60 burst), `/api/v1/keys/*`, `/api/v1/demo/*`. | Live test: 80 burst resulted in 61x 200, 19x 429 |
| **Input Sanitization (Rule 22)** | Amount validated (1 to 100,000,000,000 cents). Currency code strictly validated to 3-char alphabetic ISO. Multiplier clamped. | Live test: 400 Bad Request on malformed inputs |
| **SQL Injection (Rule 24)** | Compiled, parameterized queries via `sqlx` (`$1`, `$2`). No string concatenation. | Code audit of all database queries |
| **Webhook Integrity (Rule 64)** | Razorpay webhooks enforce HMAC-SHA256 signature verification via secret key. | Verified in `crates/esa-razorpay` |
| **API Versioning (Rule 5)** | Dual-mounted `/api/v1/` routes with backwards-compatible `/api/` aliases. | Verified across all 25+ route paths |
| **Audit Trails (Rule 54)** | Cryptographic SHA-256 hash-chain verification on decision and action history. | Verified via `/api/v1/audit/verify-chain` |

---

## 4. Security Gap Assessment

Against the 79-point specification, the following areas require hardening:

```text
┌─────────────────────────────────┬──────────┬────────────────────────────────────────────────────────┐
│ Area                            │ Severity │ Description                                            │
├─────────────────────────────────┼──────────┼────────────────────────────────────────────────────────┤
│ Distributed Rate Limiting       │ HIGH     │ In-memory Governor limits do not share state across    │
│ (Rule 13)                       │          │ horizontal multi-instance deployments without Redis.   │
├─────────────────────────────────┼──────────┼────────────────────────────────────────────────────────┤
│ Concurrency Limiting (Rule 12)  │ HIGH     │ No hard ceiling on maximum concurrent in-flight        │
│                                 │          │ expensive AI diagnosis queries.                        │
├─────────────────────────────────┼──────────┼────────────────────────────────────────────────────────┤
│ Circuit Breaker (Rule 17)       │ MEDIUM   │ Fallback exists for Ollama, but no explicit circuit-    │
│                                 │          │ trip counter (tripping closed after N consecutive 5xx).│
├─────────────────────────────────┼──────────┼────────────────────────────────────────────────────────┤
│ Cost / Token Quotas (Rule 14)   │ MEDIUM   │ Token metrics tracked, but no hard monthly budget cap  │
│                                 │          │ that halts inference when a tenant reaches limit.      │
├─────────────────────────────────┼──────────┼────────────────────────────────────────────────────────┤
│ Security Headers (Rule 65)      │ MEDIUM   │ Axum serves permissive CORS without explicit CSP,      │
│                                 │          │ HSTS, and X-Content-Type-Options headers.              │
├─────────────────────────────────┼──────────┼────────────────────────────────────────────────────────┤
│ Prompt Injection Defense        │ MEDIUM   │ Agent system prompts can be further hardened with      │
│ (Rule 33-34)                    │          │ strict boundary delimiters and output format checks.   │
├─────────────────────────────────┼──────────┼────────────────────────────────────────────────────────┤
│ SSRF Guard on Outgoing URLs     │ LOW      │ Gateway URLs currently configured via environment,     │
│ (Rule 23)                       │          │ but dynamic webhook dispatch needs private IP filter.  │
└─────────────────────────────────┴──────────┴────────────────────────────────────────────────────────┘
```

---

## 5. Detailed Risk Analysis

### Critical & High Risks

#### Risk 1: Horizontal Scaling Rate Limit Drift (High)
- **Impact**: In a multi-replica container deployment (e.g. 3 Railway instances), in-memory rate limiting allows 3x the burst threshold because counters are local to each container.
- **Remediation**: Add optional Redis connection support (`REDIS_URL` in `.env`) for distributed sliding-window counters when scaled horizontally.

#### Risk 2: Concurrency Flooding on AI Engine (High)
- **Impact**: If 100 concurrent checkout failovers occur simultaneously, the `DiagnosisAgent` could overwhelm the 16GB Ollama instance with concurrent inference requests.
- **Remediation**: Wrap `OllamaClient` in a `tokio::sync::Semaphore` (maximum 4 concurrent inference tasks).

---

### Medium Risks

#### Risk 3: HTTP Security Headers in API & Web Server (Medium)
- **Impact**: Without explicit security headers, web browsers do not enforce strict MIME-type sniffing prevention, framing restrictions, or referrer leakage protection.
- **Remediation**: Attach Tower-HTTP security headers middleware (`X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Strict-Transport-Security`, `Referrer-Policy: strict-origin-when-cross-origin`).

#### Risk 4: AI Output Strict Schema Validation (Medium)
- **Impact**: If an LLM hallucinates unexpected keys in JSON output, the JSON parser might fail or ingest unexpected fields.
- **Remediation**: Use `serde(deny_unknown_fields)` on agent diagnosis and proposal structs.

---

## 6. Implementation & Remediation Roadmap

### Phase 1: Immediate Hardening (Current Sprint)
1. **Security Headers Middleware**: Attach standard HTTP security headers to all Axum responses (`X-Content-Type-Options`, `X-Frame-Options`, `HSTS`).
2. **AI Inference Concurrency Semaphore**: Limit Ollama concurrent tasks to a configurable ceiling (`OLLAMA_MAX_CONCURRENCY=4`) to prevent worker starvation.
3. **Agent Circuit Breaker**: Add a simple circuit breaker state to Ollama client (open for 30s after 5 consecutive failures).
4. **Agent Output Schema Enforcement**: Add strict serde validation to prevent unexpected model output fields.

### Phase 2: Distributed Hardening (Production Cluster)
1. **Redis Rate Limiting**: Wire `REDIS_URL` into distributed rate-limiting middleware for multi-instance deployments.
2. **Tenant Budget Quotas**: Enforce monthly token and transaction limits stored in Supabase `public.merchants`.
3. **SSRF Outbound Validator**: Ensure webhook notifications reject private IP addresses (`127.0.0.1`, `169.254.169.254`, `10.0.0.0/8`).

---

## 7. Compliance Verification Checklist (Section 75)

- [x] Provider credentials are server-side only
- [x] API keys are high entropy
- [x] API keys are hashed (SHA-256)
- [x] API key rotation supported via Supabase ledger
- [x] API key revocation supported
- [x] API scopes supported (`scopes TEXT[]`)
- [x] Tenant identity is server-derived from JWT / key
- [x] Cross-tenant access prevented by Supabase RLS
- [x] Rate limiting exists (Token-bucket via Governor)
- [x] Request bounds & input sanitization implemented
- [x] Parameterized SQL queries (No SQL injection)
- [x] Deterministic Policy Engine (LLM is not authorization authority)
- [x] Agents have distinct identities (`monitor`, `diagnosis`, `planning`, `safety`)
- [x] API versioning enforced (`/api/v1/`)
- [x] Immutable cryptographic audit trail implemented
- [x] Zero hardcoded secrets in source code
