# AI API & Agent Platform — Security Hardening Specification

**Document Type:** Security Architecture + Implementation Specification  
**Audience:** Coding agents, backend engineers, DevSecOps engineers, security reviewers  
**Priority:** Security-first  
**Objective:** Harden the complete AI/API platform against API abuse, credential theft, tenant breakout, prompt injection, excessive agent permissions, malicious files, supply-chain vulnerabilities, data exfiltration, upstream API abuse, and compromised components.

---

# 1. Core Security Principle

The system MUST NOT depend on the AI model itself as a security boundary.

The architecture MUST assume:

```text
The user can be malicious.
The uploaded file can be malicious.
The retrieved document can be malicious.
The model can be manipulated.
The tool output can be malicious.
A dependency can contain a vulnerability.
An API credential can eventually leak.
An individual service can eventually be compromised.
```

Security must therefore come from **independent deterministic controls** such as:

```text
Authentication
Authorization
Tenant isolation
Least privilege
Rate limiting
Input validation
Output validation
Sandboxing
Network isolation
Secret management
Credential expiration
Audit logging
Monitoring
Dependency scanning
Incident response
```

The LLM MUST NOT be considered an authorization authority.

---

# 2. Security Architecture

The target architecture is:

```text
                         INTERNET
                            |
                            v
                 ┌─────────────────────┐
                 │ WAF / DDoS / CDN    │
                 └──────────┬──────────┘
                            |
                            v
                 ┌─────────────────────┐
                 │ API Gateway         │
                 │                     │
                 │ Authentication      │
                 │ API-key validation  │
                 │ JWT validation      │
                 │ IP protection       │
                 │ Rate limiting       │
                 │ Request limits      │
                 └──────────┬──────────┘
                            |
                            v
                 ┌─────────────────────┐
                 │ Application API     │
                 │                     │
                 │ Authorization       │
                 │ Tenant isolation    │
                 │ Input validation    │
                 │ Business rules      │
                 └──────────┬──────────┘
                            |
            ┌───────────────┼─────────────────┐
            |               |                 |
            v               v                 v
         Redis           Database        Object Storage
       quotas/cache      metadata         uploads
            |                                 |
            v                                 v
      Concurrency /                    Quarantine /
      Queue Manager                    scanning
            |
            v
       AI Orchestrator
            |
            v
      Policy / Tool Guard
            |
       ┌────┴────────────┐
       |                 |
       v                 v
   Model Provider    Agent Runtime
                         |
                         v
                    Tool Services
                         |
                         v
                    Sandboxed Worker
                         |
                         v
                  Restricted Network
```

---

# 3. Mandatory Security Rules

The following rules are mandatory.

## Rule 1 — Never expose provider secrets

Provider credentials such as:

```text
OPENAI_API_KEY
GOOGLE_API_KEY
ANTHROPIC_API_KEY
AWS_SECRET_ACCESS_KEY
GITHUB_TOKEN
DATABASE_PASSWORD
```

MUST NEVER be exposed to:

```text
browser JavaScript
mobile clients
frontend source
LLM context
logs
error responses
public configuration
Git repositories
```

They MUST be stored in a secure secrets manager.

---

## Rule 2 — The model must never make authorization decisions

Bad:

```text
User
 ↓
LLM
 ↓
LLM decides whether action is allowed
 ↓
Tool
```

Required:

```text
User
 ↓
LLM proposes action
 ↓
Authorization / Policy Engine
 ↓
Server-side identity verification
 ↓
Tool authorization
 ↓
Tool execution
```

The model can propose an action.  
The application decides whether that action is permitted.

---

## Rule 3 — Least privilege everywhere

Every:

```text
user
API key
service
agent
worker
integration
database account
OAuth token
container
```

MUST receive only the permissions necessary for its job.

---

## Rule 4 — Compromise must be contained

If one component is compromised:

```text
image worker
AI agent
API key
user account
third-party integration
dependency
```

the attacker MUST NOT automatically gain access to:

```text
all tenants
production database
provider secrets
internal services
employee systems
other users
all integrations
```

---

# 4. Threat Model

The system MUST account for at least the following threats:

```text
1. Stolen API keys
2. Credential stuffing
3. Brute-force authentication
4. API abuse
5. DDoS / resource exhaustion
6. Provider quota exhaustion
7. Prompt injection
8. Indirect prompt injection
9. Malicious RAG documents
10. RAG poisoning
11. Tool abuse
12. Excessive agent permissions
13. Cross-tenant data access
14. Sensitive-data leakage
15. Malicious file uploads
16. Malicious image/PDF parsing
17. Remote code execution in workers
18. Supply-chain vulnerabilities
19. Dependency compromise
20. Session/token theft
21. OAuth abuse
22. SSRF
23. SQL injection
24. Command injection
25. Path traversal
26. Unauthorized data export
27. Model output injection
28. Queue flooding
29. Cost abuse
30. Insider misuse
```

---

# 5. Authentication

## API Keys

API keys SHOULD have a structure similar to:

```text
svc_live_<high_entropy_random_value>
```

Example:

```text
svc_live_8f72c9...
```

DO NOT use predictable identifiers such as `api_key_001`, `user123_key`, or `company_name_key`.

---

## Store hashes, not plaintext keys

Database:

```text
api_key_id
tenant_id
key_hash
key_prefix
scopes
created_at
expires_at
revoked_at
last_used_at
```

Do not store `full_api_key`. The system should only reveal the complete API key once at creation.

---

# 6. API Key Lifecycle

Support:

```text
Create
List
Rotate
Revoke
Expire
Disable
```

Recommended rotation:

```text
Old key
   |
Create new key
   |
Deploy new key
   |
Verify
   |
Revoke old key
```

Do not require customers to destroy the old key before they have a replacement.

---

# 7. API Key Scopes

Every API key SHOULD support scopes.

Example:

```text
models:read
generate:write
files:read
files:write
analytics:read
```

This key must not be able to `manage_users`, `delete_tenant`, `manage_billing`, or `access_admin` unless explicitly granted.

---

# 8. Tenant Isolation

Every request MUST map to an authenticated tenant.

Never trust client-supplied tenant IDs (`{ "tenant_id": "tenant_b" }`).

Instead:

```text
API key
  ↓
Authenticated principal
  ↓
Server-side tenant ID
  ↓
Authorization
  ↓
Database query
```

Database access should enforce:

```sql
WHERE tenant_id = authenticated_tenant_id
```

The client MUST NOT be able to change tenant identity simply by changing a request field.

---

# 9. Cross-Tenant Security

Every resource MUST be associated with its owning tenant:

```text
users, projects, files, conversations, API keys, jobs, models, documents, embeddings, usage records, logs
```

Every access path must validate: `Does this resource belong to the authenticated tenant?`  
Do not rely solely on frontend filtering.

---

# 10. Authentication and Authorization Must Be Separate

- **Authentication**: *Who are you?*
- **Authorization**: *What are you allowed to do?*

Both checks are required.

---

# 11. Rate Limiting

Use multiple dimensions of rate limiting:
- **IP-based**: Protect against anonymous abuse.
- **API-key-based**: Enforce per-client limits.
- **Tenant-based**: Enforce per-tenant daily/monthly limits.
- **Endpoint-based**: Weight limits by computational expense (e.g. Health = 1, Chat = 5, Batch = 50).

---

# 12. Concurrency Limits

Enforce maximum concurrent requests per tenant to prevent slow requests from exhausting worker pools.

---

# 13. Distributed Rate Limiting

If there are multiple backend instances, rate limiting counters must reside in a shared store such as Redis.

---

# 14. Token / Cost Quotas

Track requests, input/output tokens, image generations, audio duration, compute time, storage, and bandwidth against monthly budgets.

---

# 15. Upstream Provider Rate Limits

Distinguish service quota from upstream provider quota. Handle upstream limits using queuing, caching, request deduplication, concurrency control, and backoff. Never attempt to circumvent quotas through unauthorized key rotation.

---

# 16. Retry Strategy

Use exponential backoff with random jitter on 429/5xx responses. Honor `Retry-After` headers and cap maximum retry counts.

---

# 17. Circuit Breaker

Trip a circuit breaker when upstream providers fail repeatedly to prevent retry storms.

---

# 18. Queueing

For asynchronous workloads, return `202 Accepted` with a `job_id` and process via workers.

---

# 19. Cache

Cache safe deterministic data (metadata, public config). Never cache private user data or authorization decisions.

---

# 20. Request Deduplication

Coalesce concurrent identical in-flight expensive requests to avoid duplicate compute.

---

# 21. WAF / Edge Protection

Edge WAF protects against DDoS, bot floods, and volumetric IP abuse. Does not replace application authorization.

---

# 22. Request Validation

Validate HTTP method, content type, body size, field length/type, allowed enums, array lengths, and timeouts early.

---

# 23. SSRF Protection

Block outbound requests to localhost, private IP ranges (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `127.0.0.1`), cloud metadata services (`169.254.169.254`), and internal DNS zones.

---

# 24. SQL Injection Protection

Use parameterized queries and prepared statements exclusively.

---

# 25. Command Injection Protection

Never pass raw model or user text directly to shell or subprocesses. Use command allowlists and argument validation.

---

# 26. File Upload Security

Treat uploaded files as untrusted: quarantine, magic-byte inspection, sandboxed processing, and non-executable storage.

---

# 27. File Type Validation

Inspect file signature/magic bytes, never relying solely on filename extensions or Content-Type headers.

---

# 28. Sandboxed File Processing

Run dangerous parsers (image decoders, PDF parsers, FFmpeg) in isolated, unprivileged containers with read-only filesystems.

---

# 29. Archive Extraction

Protect against zip bombs, decompression bombs, and path traversal with strict size, file count, and recursion limits.

---

# 30. Secrets Management

Use centralized secret management (AWS Secrets Manager, GCP Secret Manager, Vault, or secure server environment variables). Never commit secrets to source control or frontend bundles.

---

# 31. Secret Rotation

Support zero-downtime rotation for all provider keys, database credentials, and signing tokens.

---

# 32. Logging Rules

Never log raw credentials, authorization tokens, passwords, or session cookies. Redact sensitive headers in logs.

---

# 33. AI Prompt Injection

Treat user input, tool outputs, and external content as untrusted data. Maintain separation between system policy and user data.

---

# 34. Indirect Prompt Injection

Defend against malicious instructions embedded in retrieved documents, search results, or uploaded text.

---

# 35. RAG Security

Enforce tenant isolation and document-level authorization filtering before documents are provided to the LLM context.

---

# 36. RAG Poisoning Protection

Track document provenance, hash, ingestion timestamps, and allow quarantine or reindexing of compromised documents.

---

# 37. Agent Identity

Give each agent a distinct service identity (e.g., `agent.monitor`, `agent.diagnosis`, `agent.planning`, `agent.safety`) rather than a single universal admin token.

---

# 38. Tool Authorization

Every tool invocation must be verified by a deterministic policy engine checking tenant, user, scope, and risk before execution.

---

# 39. Tool Allowlisting

Explicitly register and allowlist authorized tools. Deny unknown tools by default.

---

# 40. High-Risk Actions

Require elevated approval, two-person rule, or step-up authentication for high-risk actions (e.g. database purge, financial transfer).

---

# 41. Never Expose Production Credentials to Agents

Do not pass master database passwords or provider root keys to agent contexts. Use narrowly scoped, short-lived tokens.

---

# 42. Short-Lived Credentials

Use short-lived credentials (e.g. 5-minute scoped tokens) for agent operations.

---

# 43. Agent Sandboxing

Sandboxes for code execution or file manipulation must have resource limits, network restrictions, non-root users, and ephemeral filesystems.

---

# 44. Network Egress

Enforce default-deny egress filtering for high-risk worker environments.

---

# 45. Output Validation

Validate all model outputs against strict schemas before executing any downstream actions.

---

# 46. Structured Outputs

Require models to produce strict, validated JSON schemas for tool arguments.

---

# 47. Database Security

Enforce least-privileged accounts, TLS in transit, tenant isolation, and encrypted backups.

---

# 48. Database Roles

Separate administrative migration roles from application read/write and analytics read-only roles.

---

# 49. Production Environment Isolation

Isolate development, staging, and production environments and secrets.

---

# 50. CI/CD Security

Automate SAST, dependency scanning, secret scanning, and container scanning on every pull request.

---

# 51. Dependency Security

Maintain lockfiles, SBOM, and continuously scan and patch third-party dependencies.

---

# 52. Container Security

Run containers as non-root, drop unused Linux capabilities, enforce CPU/memory limits, and scan images.

---

# 53. Observability

Monitor request rates, 429 counts, auth failures, token usage, latency, queue depths, and policy violations.

---

# 54. Immutable Audit Logs

Record structured audit events with cryptographic integrity for all sensitive operations.

---

# 55. Security Alerts

Alert on credential anomalies, authorization spikes, token consumption spikes, and worker crashes.

---

# 56. Anomaly Detection

Establish baseline traffic and alert on sudden behavioral deviations.

---

# 57. Cost Protection

Cap maximum tokens, prompt sizes, execution timeouts, recursion depths, and tool calls per execution.

---

# 58. Agent Loop Protection

Enforce strict limits on turn counts, tool calls, and execution timeouts to prevent infinite loops.

---

# 59. BYOK Security

Encrypt customer-provided API keys using envelope encryption or a secure secrets manager.

---

# 60. Provider Key Isolation

Strictly isolate BYOK credentials so Tenant A cannot access or route through Tenant B's credentials.

---

# 61. Do Not Circumvent Provider Quotas

Comply with upstream provider terms using caching, queuing, backoff, and approved capacity scaling.

---

# 62. Error Handling

Return generic, safe error messages to clients without leaking internal stack traces or database credentials.

---

# 63. Session Security

Use HttpOnly, Secure, SameSite cookies with short lifetimes and session rotation for web clients.

---

# 64. OAuth / OIDC Security

Validate issuer, audience, signatures, state, redirect URIs, and enforce PKCE.

---

# 65. Security Headers

Configure CSP, HSTS, X-Content-Type-Options, Referrer-Policy, and frame restrictions.

---

# 66. TLS

Enforce TLS 1.2+ for all public API endpoints and sensitive internal communication.

---

# 67. Data Encryption

Encrypt data in transit (TLS) and at rest (AES-256 / KMS).

---

# 68. Data Minimization

Collect and retain only necessary operational data with defined retention policies.

---

# 69. Security Testing

Test invalid/expired credentials, cross-tenant bypass, rate limit burst, prompt injection, and malformed inputs.

---

# 70. Red-Team Scenarios

Simulate credential theft, tenant switching, prompt injection, RAG poisoning, and queue exhaustion attacks.

---

# 71. Security Failure Philosophy

Always fail closed (deny on error, timeout, or ambiguity).

---

# 72. Incident Response

Establish containment, credential revocation, isolation, patching, and restoration procedures.

---

# 73. Emergency Controls

Provide instant revocation of API keys, tenants, tools, or providers without requiring code redeployments.

---

# 74. Security Dashboard

Track active keys, 429 rates, auth failures, cost, tool invocations, and policy verdicts in real time.

---

# 75. Definition of Done

Validate against all 39 checklist requirements in Section 75 before declaring security hardening complete.

---

# 76. Required Agent Workflow

1. Inspect repository structure.
2. Identify backend architecture.
3. Identify frontend architecture.
4. Identify authentication implementation.
5. Identify API-key implementation.
6. Identify database architecture.
7. Identify Redis/cache usage.
8. Identify AI provider integrations.
9. Identify agent/tool architecture.
10. Identify file-upload pipeline.
11. Identify secret-management strategy.
12. Identify deployment architecture.
13. Identify logging/monitoring.
14. Identify existing security controls.

Then produce `SECURITY_GAP_REPORT.md`.

---

# 77. Implementation Priority

- **P0 — Critical**: Authentication, Authorization, Tenant isolation, Secret protection, API key security, Provider key protection, Agent/tool authorization, SSRF protection, Command execution isolation, Database access control, Sensitive logging prevention.
- **P1 — High**: Redis rate limiting, Concurrency control, Quotas, Queueing, Circuit breaker, Retry/backoff, Audit logs, Security alerts, Dependency scanning, Container hardening, RAG isolation, Prompt injection defenses.
- **P2 — Advanced**: Behavior anomaly detection, Automated security response, Advanced policy engine, Short-lived agent credentials, Advanced sandboxing, Continuous red-team testing.

---

# 78. Required Implementation Output

1. Security gap report (`SECURITY_GAP_REPORT.md`).
2. Architecture changes.
3. Files changed.
4. Database migrations.
5. Environment variables added.
6. Secret-management changes.
7. API security changes.
8. Agent security changes.
9. File-processing security changes.
10. Provider-quota handling.
11. Security tests and results.
12. Remaining risks.
13. Deployment instructions.
14. Rollback instructions.

---

# 79. Final Architecture Principle

> **Do not try to make the model "behave securely" through prompts alone. Build deterministic authorization, isolation, least privilege, validation, rate limiting, secret management, and containment around the model.**
