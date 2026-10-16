# ESAPay Threat Model & Security Architecture (Phase 0)

**Document Version:** 1.0.0  
**Target Specification:** STRIDE / NIST SP 800-53 / Payment Gateway Threat Modeling  
**Date:** October 2026  
**System:** ESAPay Autonomous Payment Gateway & Executable State Architecture  

---

## 1. System Overview & Trust Boundaries

ESAPay is an autonomous payment routing and self-healing resilience platform. It ingests transaction events, monitors workload vitals, routes checkouts across multiple payment corridors (Razorpay, Stripe, PhonePe, Paytm, Cashfree), and executes closed-loop recovery via an AI/rule-assisted control plane.

```text
       UNTRUSTED INTERNET / CLIENTS
   ┌─────────────────────────────────────────────────────────┐
   │ Public Browsers, Fraudulent Callers, Impersonators       │
   └──────────────────────────┬──────────────────────────────┘
                              │
  [TRUST BOUNDARY 1: External API / Network Ingress]
                              │ HTTPS / WSS
                              ▼
   ┌─────────────────────────────────────────────────────────┐
   │ Axum API Gateway (crates/esa-api)                       │
   │  - Origin / CORS Filtering                              │
   │  - Request Size & Rate Limiter                          │
   │  - Authentication & Route Authorization Middleware     │
   └─────────────┬───────────────────────────┬───────────────┘
                 │                           │
  [TRUST BOUNDARY 2: Core State & Autonomous Engine]
                 │                           │
                 ▼                           ▼
   ┌───────────────────────────┐ ┌───────────────────────────┐
   │ StateFabric (State Engine)│ │ Multi-Agent Control Plane │
   │  - Workload Entity Vitals │ │  - Monitor Agent          │
   │  - OCC State Versions     │ │  - Diagnosis Agent        │
   │  - Snapshots & Rollbacks  │ │  - Planning Agent         │
   └─────────────┬─────────────┘ └───────────┬───────────────┘
                 │                           │
  [TRUST BOUNDARY 3: Deterministic Policy & Safety Gate]
                 │                           │
                 └─────────────┬─────────────┘
                               ▼
   ┌─────────────────────────────────────────────────────────┐
   │ PolicyEngine & DecisionVerifier (crates/esa-policy)     │
   │  - RULE_001..004 Invariant Enforcers                    │
   │  - Strict Pre-Commit OCC Version Verification           │
   │  - Action Risk Scoring & Approval Gating                │
   └───────────────────────────┬─────────────────────────────┘
                               │
  [TRUST BOUNDARY 4: Action Execution & External Mutation]
                               │
                               ▼
   ┌─────────────────────────────────────────────────────────┐
   │ ActionGateway (crates/esa-gateway)                      │
   │  - Typed Action IR Execution (CREATE_REPLICA, etc.)     │
   │  - Optional / Guarded Kubernetes Scale                  │
   │  - Cryptographic SHA-256 Audit Store                    │
   └───────────────────────────┬─────────────────────────────┘
                               │
  [TRUST BOUNDARY 5: External Providers & Infrastructure]
                               │
                 ┌─────────────┴─────────────┐
                 ▼                           ▼
   ┌───────────────────────────┐ ┌───────────────────────────┐
   │ External Corridors        │ │ Cluster / Host Infra      │
   │ (Razorpay, Stripe, etc.)  │ │ (K8s API, Docker, DB)     │
   └───────────────────────────┘ └───────────────────────────┘
```

---

## 2. Assets & Sensitive Data Classification

| Asset Category | Specific Data / Resource | Confidentiality | Integrity | Availability | Storage / Transit Location |
|---|---|---|---|---|---|
| **Authentication Secrets** | Master API keys, Webhook secrets, JWT signing secrets | **Critical** | **Critical** | **High** | Environment variables, `.env` (Never client bundles) |
| **Merchant Credentials** | Merchant API key hashes, scopes, tenant UUIDs | **High** | **Critical** | **High** | Supabase DB / In-memory registry (Hashed only) |
| **Payment References** | Order IDs, Payment IDs, masked references (`RZP-...`) | **Medium** | **Critical** | **Critical** | StateFabric, Payment Events, Telemetry |
| **Cardholder / PII Data** | Raw card numbers (PAN), CVVs, customer identities | **N/A (Prohibited)** | **N/A** | **N/A** | **Zero-retention policy**. Tokenized by payment provider. |
| **Audit Ledger** | Decision trail, Policy verdicts, Hash-chain blocks | **Medium** | **Critical** | **High** | AuditStore (SHA-256 chained, append-only) |
| **Infrastructure State** | Replica counts, routing allocations, workload vitals | **Low** | **Critical** | **Critical** | StateFabric |

---

## 3. Threat Actors & Capabilities

1. **Anonymous Internet Attacker:**
   - Capabilities: Can send arbitrary HTTP/WebSocket requests, forge headers, replay webhooks, flood endpoints with volumetric traffic, or attempt brute-force token generation.
2. **Untrusted / Compromised Merchant Tenant:**
   - Capabilities: Holds valid test or live API keys for a specific tenant. May attempt privilege escalation, cross-tenant data access, or resource starvation via benchmark triggers.
3. **Malicious Webhook Spoofer:**
   - Capabilities: Attempts to send fake payment captured/failed events to `/api/v1/razorpay/webhook` without valid cryptographic signatures.
4. **Adversarial / Hallucinated LLM Output:**
   - Capabilities: Generates out-of-bounds replica counts, invalid action types, malicious prompt injection reflections, or stale state references in diagnosis/planning transcripts.
5. **Compromised Host / Operator:**
   - Capabilities: Direct access to local filesystem, shell, or Kubernetes cluster.

---

## 4. Threat Matrix (STRIDE Analysis)

### 4.1 Spoofing Identity

| Threat ID | Target | Attack Scenario | Current Status | Required Mitigation |
|---|---|---|---|---|
| **STRIDE-S1** | JWT Authentication (`/api/v1/auth/me`) | Attacker crafts a token with `"sub": "admin"` and no signature. `parse_supabase_jwt` decodes base64 payload without verifying cryptographic signature. | **CONFIRMED VULNERABLE (P0)** | Cryptographic verification via `jsonwebtoken` with HMAC-SHA256 secret or JWKS public keys. |
| **STRIDE-S2** | API Key Validation (`is_key_authorized`) | Attacker sends `Authorization: Bearer esa_live_sec_1111111111111111111111111`. Prefix-based check accepts any 32-character string. | **CONFIRMED VULNERABLE (P0)** | Hash-based lookup in a verified key registry with explicit revocation and expiry checks. |
| **STRIDE-S3** | Razorpay Webhook (`/api/v1/razorpay/webhook`) | Attacker crafts webhook payload and signs it using the API key secret instead of the webhook secret. | **CONFIRMED VULNERABLE (P0)** | Remove `verify_signature_any`. Only accept valid HMAC matching dedicated `RAZORPAY_WEBHOOK_SECRET`. |

### 4.2 Tampering with Data

| Threat ID | Target | Attack Scenario | Current Status | Required Mitigation |
|---|---|---|---|---|
| **STRIDE-T1** | Gateway State (`/api/gateways/:name/toggle`) | Anonymous caller flips corridor health status to degraded, disrupting live transactions. | **CONFIRMED VULNERABLE (P0)** | Require admin authentication and authorization scopes on all gateway control endpoints. |
| **STRIDE-T2** | Workload Vitals (`/api/workloads`, `/api/demo/*`) | Anonymous caller posts arbitrary workloads or triggers artificial spikes on all corridors. | **CONFIRMED VULNERABLE (P0)** | Protect `/api/workloads` (POST) with admin auth; disable demo/scenario endpoints in live mode. |
| **STRIDE-T3** | Payment State Transitions (`payment_entity_to_event`) | Provider returns unknown status; gateway maps unknown status to `PaymentCaptured`. | **CONFIRMED VULNERABLE (P0)** | Map unknown statuses strictly to `Unknown` / `Pending`. Fail closed. |

### 4.3 Repudiation

| Threat ID | Target | Attack Scenario | Current Status | Required Mitigation |
|---|---|---|---|---|
| **STRIDE-R1** | Autonomous Actions | ActionGateway executes mutation, but memory is lost on restart; no durable record of who authorized it. | **VULNERABLE (P1)** | Wire durable audit logs to persistent storage; record actor, proposal ID, and trace ID. |

### 4.4 Information Disclosure

| Threat ID | Target | Attack Scenario | Current Status | Required Mitigation |
|---|---|---|---|---|
| **STRIDE-I1** | Frontend Assets | Build bundle includes `VITE_ESA_LIVE_API_KEY`, exposing live production secret keys to anyone inspecting client JS. | **CONFIRMED VULNERABLE (P0)** | Scrub all live secrets from frontend configuration templates. |
| **STRIDE-I2** | URL Query Parameters | Frontend reads `?key=...` or `?api_key=...` from URL and saves to `localStorage`, leaking key into browser history and logs. | **CONFIRMED VULNERABLE (P0)** | Disallow passing secrets in query parameters. |
| **STRIDE-I3** | WebSocket Stream (`/ws/telemetry`) | Unauthenticated WebSocket client connects to `/ws/telemetry` and reads internal metrics and agent decisions. | **CONFIRMED VULNERABLE (P0)** | Authenticate WebSocket connection during HTTP upgrade handshake. |

### 4.5 Denial of Service

| Threat ID | Target | Attack Scenario | Current Status | Required Mitigation |
|---|---|---|---|---|
| **STRIDE-D1** | Benchmark Harness (`/api/v1/benchmark/harness`) | Anonymous caller repeatedly requests full benchmark execution, which performs CPU-intensive runs and writes files to disk. | **CONFIRMED VULNERABLE (P0)** | Restrict benchmark routes to authenticated admins or sandbox mode only. |
| **STRIDE-D2** | Ollama LLM Engine | Concurrency flood exhausts local model server memory, causing crash and fallback degradation. | **VULNERABLE (P1)** | Concurrency ceiling via `tokio::sync::Semaphore` and circuit breaker. |

### 4.6 Elevation of Privilege

| Threat ID | Target | Attack Scenario | Current Status | Required Mitigation |
|---|---|---|---|---|
| **STRIDE-E1** | Infrastructure (`kubectl`) | Control plane spawns `kubectl scale` by default because `KUBERNETES_ENABLED` defaults to true. | **CONFIRMED VULNERABLE (P0)** | Default `KUBERNETES_ENABLED` to false. Require deliberate opt-in and strict namespace limits. |
| **STRIDE-E2** | LLM Prompt Injection | Adversarial prompt in payment notes attempts to force agent to execute arbitrary actions. | **MITIGATED IN IR (Low)** | Agents output typed Action IR only (`ActionType` enum); no free-form shell/exec actions exist. |

---

## 5. Security Architecture Invariants

1. **Deterministic Authority:** The LLM is an advisory component only. Every proposed mutation must be verified against deterministic policy rules (`RULE_001` - `RULE_004`) and commit-time optimistic concurrency checks (OCC).
2. **Fail-Closed Authorization:** Every mutating endpoint must reject unauthenticated requests. Missing credentials must never result in an anonymous fallback.
3. **No Secrets in Public Tiers:** Neither live API keys nor provider secrets may ever exist in Vite bundles, public HTML, or URL query strings.
4. **Infrastructure Protection:** Infrastructure mutation (e.g. Kubernetes scaling) is strictly opt-in and disabled by default.

---

*Threat Model completed under Phase 0 criteria. Ready for Phase 1 P0 Implementation.*
