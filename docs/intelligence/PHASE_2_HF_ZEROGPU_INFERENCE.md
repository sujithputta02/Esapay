# ESAPay Phase 2 — Model Provider Abstraction & Hugging Face ZeroGPU Integration

**Branch:** `feature/phase-2-hf-zerogpu-provider` (branched from `main` after Phase 1 PR #25 merge)  
**Guiding Principle:** ESAPay owns its intelligence architecture. Hugging Face ZeroGPU supplies experimental inference capacity; the Rust application retains privacy, validation, policy, and financial authority.

---

## 1. Architecture & Provider Abstraction

The previous direct coupling to `OllamaClient` in `DiagnosisAgent`, `EsaOrchestrator`, and `AppState` has been refactored behind a provider-independent interface in `crates/esa-agents/`:

- [`provider.rs`](../../crates/esa-agents/src/provider.rs):
  - `InferenceProvider` (`async_trait`): `provider_kind()`, `model_id()`, `infer(InferenceRequest) -> Result<InferenceResponse, InferenceError>`, `cost_tracker()`, `token_stats()`.
  - `ExecutionScope`: `System` vs. `MerchantScoped { merchant_id }` (explicit tenant propagation; never invents a merchant ID).
  - `GenerationLimits`: `max_prompt_chars`, `max_new_tokens`, `max_output_bytes`, `temperature`, `top_p`.
  - `InferenceError` + `InferenceErrorCategory`: sanitized, typed error categories (`Unauthorized`, `Forbidden`, `NotFound`, `Timeout`, `RateLimited`, `QuotaExhausted`, `ServiceUnavailable`, `CircuitOpen`, `BadGateway`, `InvalidResponse`, `PromptTooLarge`, `Cancelled`, `Misconfigured`, `ConnectionError`, `UpstreamError`).
  - `AiProviderConfig`: startup configuration loader (`ESA_AI_PROVIDER`, `OLLAMA_*`, `HF_AI_*`).
- [`ollama_provider.rs`](../../crates/esa-agents/src/ollama_provider.rs):
  - `OllamaProvider` adapts `OllamaClient` behind `InferenceProvider`, preserving local development compatibility, tenant-scoped caching, and `AICostTracker` telemetry.
- [`hf_zerogpu.rs`](../../crates/esa-agents/src/hf_zerogpu.rs):
  - `HuggingFaceZeroGpuProvider` implements the Gradio 5.20.0 queue-based HTTP + SSE contract with private Space bearer authorization, concurrency semaphore, circuit breaker, bounded response reading, and queue/GPU latency tracking.
- [`inference.rs`](../../crates/esa-agents/src/inference.rs):
  - `build_inference_provider()` selects `OllamaProvider` (`ollama`, default) or `HuggingFaceZeroGpuProvider` (`hf_zerogpu`) at startup. Misconfigured remote credentials fail closed on `infer()` without crashing startup or interrupting payment execution.

---

## 2. Gradio API Contract & Hugging Face Space Updates

The mismatch between the old README (which claimed Ollama-compatible `/api/generate`, `/api/chat`, `/api/tags`, `/health`) and `deploy/huggingface-zerogpu/app.py` (Gradio Blocks `api_name="generate"`) is resolved:

1. **Submit Inference:** `POST {HF_AI_BRAIN_URL}/gradio_api/call/generate`
   - Headers: `Authorization: Bearer <HF_AI_BRAIN_TOKEN>`, `Content-Type: application/json`
   - Body: `{"data": ["<sanitized_prompt>"]}`
   - Response: `{"event_id": "..."}`
2. **Stream Inference Result:** `GET {HF_AI_BRAIN_URL}/gradio_api/call/generate/{event_id}`
   - Headers: `Authorization: Bearer <HF_AI_BRAIN_TOKEN>`, `Accept: text/event-stream`
   - Events handled: `status`, `progress`/`generating`, `complete` (`data: ["<text>"]`), `error`.
   - Malformed SSE lines, missing `event_id`, empty output, oversized responses (`> HF_AI_MAX_OUTPUT_BYTES`), and unexpected EOF fail closed.

### Model Verification (`Qwen/Qwen2.5-0.5B-Instruct`)
- **Model ID:** `Qwen/Qwen2.5-0.5B-Instruct`
- **Pinned Revision:** `7ae557604adf67be50417f59c2c2f167def9a775`
- **License:** Apache License 2.0 (`apache-2.0`)
- **Model Card URL:** https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct
- **Gradio SDK Version:** `5.20.0` (Space metadata)
- **Python Dependencies (`deploy/huggingface-zerogpu/requirements.txt`):** `transformers>=4.45.0,<5.0.0`, `accelerate>=0.34.0,<2.0.0`, `torch>=2.3.0`

---

## 3. Authentication & Privacy Guarantees

1. **Private Space Auth:** `HF_AI_BRAIN_TOKEN` (fine-grained read token for the private Space) is injected server-side via `Authorization: Bearer ...` only on backend HTTP calls.
2. **Secret Protection:** `HfZeroGpuConfig`'s `Debug` impl redacts `token` as `[REDACTED]`; `InferenceError` scrubs messages via `privacy::redact_sensitive_text`; `render.yaml` declares `HF_AI_BRAIN_TOKEN` with `sync: false`; no token is exposed in `frontend/`, Vite bundles, WebSockets, or URLs.
3. **Data Minimization & Injection Defense:** `DiagnosisAgent` builds prompts from allowlisted structured telemetry (`workload_id`, `condition_type`, `severity`, numeric `metrics` fields) wrapped in `<OBSERVED_TELEMETRY>`, sanitized by `privacy::sanitize_prompt`, and validated by `OutputValidator` before any action recommendation is accepted.
4. **Tenant Isolation:** Cache keys and `AICostTracker` metrics are scoped by `ExecutionScope` (`system:__global__` vs. `merchant:<id>`).

---

## 4. Environment Variables

| Variable | Default | Purpose |
| --- | --- | --- |
| `ESA_AI_PROVIDER` | `ollama` | Startup provider selection (`ollama` or `hf_zerogpu`) |
| `OLLAMA_URL` | `http://localhost:11434` | Local Ollama endpoint |
| `OLLAMA_MODEL` | `qwen2.5:0.5b` | Local Ollama model |
| `HF_AI_BRAIN_URL` | *(unset)* | Private Hugging Face Space base URL |
| `HF_AI_BRAIN_TOKEN` | *(unset)* | Backend-only Hugging Face read token (`sync: false` in Render) |
| `HF_AI_MODEL_ID` | `Qwen/Qwen2.5-0.5B-Instruct` | HF model identifier for telemetry |
| `HF_AI_CONNECT_TIMEOUT_SECS` | `5` | TCP/TLS connect timeout |
| `HF_AI_SUBMIT_TIMEOUT_SECS` | `10` | `POST /gradio_api/call/generate` timeout |
| `HF_AI_TOTAL_TIMEOUT_SECS` | `45` | End-to-end deadline including SSE queue/generation |
| `HF_AI_MAX_CONCURRENCY` | `2` | Local semaphore cap on concurrent ZeroGPU calls |
| `HF_AI_MAX_PROMPT_CHARS` | `8192` | Maximum prompt characters |
| `HF_AI_MAX_OUTPUT_BYTES` | `16384` | Maximum SSE/HTTP response bytes |
| `HF_AI_CIRCUIT_BREAKER_COOLDOWN_SECS` | `30` | Cooldown after 3 consecutive failures |

---

## 5. Failure Handling, Queueing & Quota Limitations

- **ZeroGPU Quota & Shared Compute:** ZeroGPU dynamically allocates shared GPU time and enforces account-level daily GPU quotas; cold starts and queue waits are normal.
- **No Blind POST Retries:** If `POST /gradio_api/call/generate` times out or fails, it is never blindly resubmitted. Only polling an already-accepted `event_id` performs bounded retries on `429`/`503` (honoring `Retry-After` strictly within `total_timeout`).
- **Deterministic Fallback:** On any provider error (`401`, `403`, `404`, `408`, `429`, `500`, `502`, `503`, `504`, timeout, malformed SSE, circuit open) or `OutputValidator` rejection, `DiagnosisAgent` falls back to `rule_based_diagnosis` and records `provider = "deterministic_fallback"` in `AICostTracker`.
- **Payment Isolation:** Checkout, confirmation, capture, refunds, and webhooks never depend on AI inference availability.

---

## 6. Manual Deployment Checklist & Local Testing

### Render Manual Deployment Checklist
1. Keep the Hugging Face Space **private**; push updated `deploy/huggingface-zerogpu/{app.py,requirements.txt,README.md}` to the Space repository.
2. Create a fine-grained Hugging Face access token with minimum read permissions for that Space.
3. In the Render dashboard for `esapay-api`, set `HF_AI_BRAIN_TOKEN` (marked `sync: false` in `render.yaml`) and confirm `ESA_AI_PROVIDER=hf_zerogpu` and `HF_AI_BRAIN_URL`.
4. Verify `/api/costs/ai` (with admin scope) reports `active_provider: "hf_zerogpu"`.

### Local Testing
- **Mocked Rust tests (no live HF credentials required):** `cargo test --workspace`
- **Local Python syntax/mock check:** `ESA_HF_MOCK_MODE=true python3 -c "import deploy.huggingface_zerogpu.app"`
- **Local Ollama workflow (default):** Leave `ESA_AI_PROVIDER=ollama` and run local Ollama on `http://localhost:11434`.

### Known Limitations
- Live authenticated inference against a private ZeroGPU Space was not executed in CI/sandbox because no `HF_AI_BRAIN_TOKEN` secret is provisioned in the local environment.
- `AICostTracker` remains process-local in-memory telemetry (persistent cross-node AI memory/storage is deferred to Phase 3).
