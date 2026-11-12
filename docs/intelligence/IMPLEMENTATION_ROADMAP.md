# ESAPay Intelligence Core — Implementation Roadmap

**Audit Date:** 2026-11-12  
**Guiding Principle:** Independently reviewable phases, zero disruption to the deterministic Rust payment path, and full test/security verification at every gate.

---

## 1. Phase Dependency Overview

```mermaid
gantt
    title ESAPay Intelligence Core Implementation Roadmap
    dateFormat  YYYY-MM-DD
    axisFormat  %b %d

    section Foundation & Safety
    Phase 1: Provider Abstraction, ZeroGPU & Privacy Guard :p1, 2026-11-16, 10d
    Phase 2: Durable State, Audit Lineage & Payment Fixes  :p2, after p1, 8d

    section Intelligence & Memory
    Phase 3: Dual-Cadence Agents & Rust Hysteresis         :p3, after p2, 10d
    Phase 4: 4-Tier Persistent Memory & Payment Advisors   :p4, after p3, 12d

    section Verification & Twin
    Phase 5: Digital Twin, Live Benchmarks & ESA-RBench    :p5, after p4, 10d
    Phase 6: Controlled Calibration & Fairness Governance  :p6, after p5, 8d
```

---

## 2. Detailed Phase Breakdown

### Phase 1: Model-Provider Abstraction, Private ZeroGPU Integration & Privacy Guard
- **Dependencies:** None (builds directly on `main`).
- **Scope:**
  1. Introduce `InferenceProvider` trait and `ProviderRouter` (`OllamaProvider`, `HuggingFaceZeroGpuProvider`, `DeterministicRuleProvider`) with `AI_PROVIDER` selection.
  2. Implement private HF Space auth (`HF_AI_BRAIN_TOKEN` backend-only), `HF_AI_BRAIN_URL` consumption, Gradio/REST protocol alignment in `deploy/huggingface-zerogpu/app.py`, queue/total timeouts, `429`/quota-exhausted circuit breaker, and Ollama/rule fallback.
  3. Implement `PromptSanitizer` (scrubs PII/PCI/secrets before inference, caching, and `AICostTracker`) and `OutputValidator` (`#[serde(deny_unknown_fields)]`, confidence clamping `0.0..=1.0`, strict enum validation).
  4. Fix `reqwest::Client::builder().no_proxy()` option (or test-safe client builder) where needed so unit tests do not panic in restricted macOS sandboxes.
- **Exact Files Likely to Change:**
  - `crates/esa-agents/src/lib.rs`, `ollama.rs`, `diagnosis.rs`, new `provider.rs`, new `hf_zerogpu.rs`, new `sanitizer.rs`
  - `crates/esa-api/src/main.rs` (provider initialization & config validation)
  - `deploy/huggingface-zerogpu/app.py`, `deploy/huggingface-zerogpu/README.md`, `deploy/huggingface-zerogpu/requirements.txt`
  - `.env.example`, `render.yaml` (document `HF_AI_BRAIN_TOKEN` as `sync: false`, `AI_PROVIDER`)
- **Acceptance Criteria:**
  - `HF_AI_BRAIN_URL` + `HF_AI_BRAIN_TOKEN` authenticate to a private ZeroGPU Space without exposing tokens to `frontend/`.
  - Simulated ZeroGPU `429`/quota/timeout triggers fallback to Ollama or deterministic rules in `<100ms` after circuit trip without interrupting any payment route.
  - `PromptSanitizer` blocks synthetic PAN/email/phone/API-key patterns in unit tests.
- **Risks & Mitigations:** ZeroGPU cold starts or daily quota exhaustion -> aggressive timeouts + 5–15 min quota cooldown + seamless fallback.
- **Testing Requirements:** Mock HTTP server tests (`wiremock`/`axum` test server) for HF `200`, `401`, `429`, `503`, timeout, malformed JSON, and `PromptSanitizer` redaction suite.

---

### Phase 2: Durable State/Audit Persistence, Reasoning Lineage & Payment Fixes
- **Dependencies:** Phase 1.
- **Scope:**
  1. Wire `crates/esa-state/src/store.rs` (`StateStore`) or extend `PostgresWebhookStore` so `StateFabric` workloads and `AuditStore` hash-chained records persist durably in PostgreSQL.
  2. Populate `AuditRecord.agent_reasoning` (`AgentReasoningTrace`) in `ActionGateway::execute_with_verdict` with redacted diagnosis/safety metadata.
  3. Replace hardcoded `/api/intent/active`, `/api/intent/violations`, and `/api/agents/status` template transcripts with live `IntentManager` and `AuditStore` data.
  4. Fix `razorpay_confirm_payment` handling of `PaymentAuthorized` vs `PaymentCaptured`, and expose `/api/v1/razorpay/refunds` using `reserve_refund` + `RazorpayAdapter::create_refund`.
- **Exact Files Likely to Change:**
  - `crates/esa-state/src/store.rs`, `crates/esa-state/src/lib.rs`
  - `crates/esa-gateway/src/executor.rs`, `crates/esa-runtime/src/orchestrator.rs`
  - `crates/esa-api/src/main.rs`
- **Acceptance Criteria:**
  - Server restart preserves `AuditStore` hash chain (`verify_chain().is_valid == true`) and `WorkloadEntity` state.
  - `razorpay_confirm_payment` does not mark `Authorized` payments as `"captured"`.
- **Risks & Mitigations:** DB latency on `AuditStore::append` -> async non-blocking persistence or transactional write with fallback.
- **Testing Requirements:** Postgres + InMemory integration tests for audit persistence, `DecisionReplayer` post-restart, and Razorpay `Authorized` confirmation negative test.

---

### Phase 3: True Dual-Cadence Orchestration & Rust Hysteresis
- **Dependencies:** Phase 1, Phase 2.
- **Scope:**
  1. Port `HysteresisFilter` (EMA + debounce window) from `arc_reasoner/chaos.py` into `crates/esa-agents/src/monitor.rs` to prevent boundary flapping.
  2. Refactor `DiagnosisAgent` & `EsaOrchestrator` to implement true **Dual-Cadence** execution: synchronous Tier-1 (`FluidReasonerClient`) / Tier-3 rules for <5ms SLA protection, with non-blocking `tokio::spawn` Tier-2 LLM deliberation via `ProviderRouter`.
  3. Implement `DecisionVerifier::verify_resource_limits` (real budget/replica/concurrency checks).
- **Exact Files Likely to Change:**
  - `crates/esa-agents/src/monitor.rs`, `diagnosis.rs`, `safety.rs`
  - `crates/esa-runtime/src/orchestrator.rs`
  - `crates/esa-policy/src/verifier.rs`
- **Acceptance Criteria:**
  - Slow/hung LLM (25s) never delays `EsaOrchestrator`'s 5s monitoring cycle or payment processing.
  - Transient 1-cycle jitter does not trigger `ActionProposal`s.
- **Risks & Mitigations:** Race between async Tier-2 proposal and newer state -> already mitigated by commit-time OCC `state_version` check in `PolicyEngine` & `ActionGateway`.
- **Testing Requirements:** Async timing tests verifying <50ms fast-path cycle under simulated 10s LLM stall; hysteresis debounce unit tests.

---

### Phase 4: Persistent 4-Tier Memory & Payment-Domain Specialist Advisors
- **Dependencies:** Phase 2, Phase 3.
- **Scope:**
  1. Add PostgreSQL tables for Working, Episodic (`ai_episodic_incidents`), Semantic (`ai_semantic_knowledge`), and Procedural (`ai_procedural_playbooks`) memory with `merchant_id` scoping and `PromptSanitizer` enforcement.
  2. Add read-only advisory specialists: **Corridor Health Advisor** (correlates PSP error codes/latency) and **Reconciliation Triage Advisor** (summarizes `unapplied` refunds / `PendingReconciliation` records).
  3. Scope `/api/agents/activity` and `AICostTracker` by tenant/admin permissions.
- **Exact Files Likely to Change:**
  - `crates/esa-state/` (new `memory_store.rs`), `crates/esa-agents/` (new `memory.rs`, `advisors.rs`)
  - `crates/esa-api/src/main.rs`
- **Acceptance Criteria:**
  - Zero cross-merchant memory leakage in multi-tenant retrieval tests.
  - Procedural memory writes rejected unless signed/approved by admin workflow.
- **Risks & Mitigations:** Memory poisoning via prompt injection -> strict schema validation + provenance + read-only procedural store for agents.
- **Testing Requirements:** Cross-tenant isolation tests, memory schema rejection tests, reconciliation triage unit tests.

---

### Phase 5: Digital Twin Sandbox, Real `SafetyTestRunner` & Live `BenchmarksView`
- **Dependencies:** Phase 3, Phase 4.
- **Scope:**
  1. Replace `SafetyTestRunner` mock (`crates/esa-policy/src/safety_runner.rs`) with real `PolicyEngine` / `ActionGateway` test execution.
  2. Build Digital Twin counterfactual replay mode (`DRY_RUN=true`) combining `StateFabric` snapshots with `arc_reasoner/esa_rbench/environment.py`.
  3. Wire `arc_reasoner/esa_rbench/baselines.py` (`LLMOnlyController`, `UngatedToolLLMController`) to real `ProviderRouter` inference (or explicitly mark deterministic baselines) and compute real ablations in `runner.py`.
  4. Replace hardcoded metrics in `frontend/src/pages/BenchmarksView.tsx`, `AgentsView.tsx`, `AuditView.tsx`, and `PolicyView.tsx` with live API queries (`/api/benchmark/latest`, `/api/benchmark/ablations`, `/api/audit/verify-chain`, `/api/audit/replay/:id`).
- **Exact Files Likely to Change:**
  - `crates/esa-policy/src/safety_runner.rs`
  - `crates/esa-api/src/benchmark.rs`, `benchmark_harness.rs`
  - `arc_reasoner/esa_rbench/runner.py`, `baselines.py`
  - `frontend/src/pages/BenchmarksView.tsx`, `AgentsView.tsx`, `AuditView.tsx`, `PolicyView.tsx`
- **Acceptance Criteria:**
  - Zero hardcoded benchmark or ablation numbers in UI or Python runner.
  - `SafetyTestRunner` fails if any `PolicyEngine` invariant is broken.
- **Testing Requirements:** End-to-end `cargo test`, `pytest` on `arc_reasoner/tests/`, `bun test --run` on `frontend/`.

---

### Phase 6: Controlled Calibration & Ethical Governance
- **Dependencies:** Phase 5.
- **Scope:**
  1. Feed `EffectMeasurement` deltas into offline calibration reports (ECE, false-positive rate, SLA impact).
  2. Add Corridor & Merchant Fairness Monitor (detects disproportionate throttling/failover bias across merchants or payment rails).
  3. Enforce `REDIS_URL` distributed rate limiting for multi-replica deployments.
- **Exact Files Likely to Change:**
  - `crates/esa-agents/`, `crates/esa-telemetry/src/metrics.rs`, `crates/esa-api/src/rate_limiter.rs`
- **Acceptance Criteria:**
  - Calibration and fairness metrics exported to Prometheus and `BenchmarksView`.
  - All 650 `adversarial_suite` trials pass with 0 unsafe mutations before any model/prompt promotion.
- **Testing Requirements:** Adversarial regression suite, multi-instance Redis rate-limit test, fairness parity test.

