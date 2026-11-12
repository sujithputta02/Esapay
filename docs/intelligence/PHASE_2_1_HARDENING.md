# ESAPay Phase 2.1 — Tenant Scope, Secret Handling & ZeroGPU Runtime Consistency

**Branch:** `feature/phase-2-hf-zerogpu-provider` (PR #26)  
**Guiding Principle:** Invalid tenant identity must never become global identity, configuration diagnostics must never expose credentials, and reported model identity must match the actual running model.

---

## 1. Problems Resolved

1. **Strict Merchant Execution Scope (`crates/esa-agents/src/provider.rs`, `ollama_provider.rs`, `hf_zerogpu.rs`):**
   - Previously, `ExecutionScope::MerchantScoped { merchant_id }` with empty/whitespace-only `merchant_id` returned `None` from `merchant_id()`, causing `cache_key_prefix()` to silently fall back to `"system:__global__"`.
   - **Fix:** Added `ExecutionScope::normalized_merchant_id()` and `ExecutionScope::cache_key_prefix() -> Result<String, InferenceError>` which fail closed with `InferenceErrorCategory::InvalidScope` on empty, whitespace-only, or delimiter/control-character merchant IDs. Both `OllamaProvider::infer` and `HuggingFaceZeroGpuProvider::infer` validate scope **before** cache lookup, network calls, or telemetry recording.
2. **AI Configuration Secret Protection (`crates/esa-agents/src/provider.rs`):**
   - Replaced `#[derive(Debug)]` on `AiProviderConfig` with a custom `fmt::Debug` implementation that redacts `hf_token` as `Some("[REDACTED]")` while preserving non-secret diagnostics.
3. **Strict Provider Configuration (`crates/esa-agents/src/provider.rs`, `inference.rs`, `crates/esa-api/src/main.rs`):**
   - Replaced `AiProviderConfig::from_env()` with strict `AiProviderConfig::try_from_env()` and `build_inference_provider_from_env()`.
   - Absent/empty `ESA_AI_PROVIDER` defaults to `ollama`; `"ollama"` and `"hf_zerogpu"` are accepted; any unsupported value returns `InferenceErrorCategory::Misconfigured`.
   - Misconfigured `hf_zerogpu` (missing URL, missing token, invalid URL, or unsupported `ESA_AI_PROVIDER`) returns `MisconfiguredProvider` rather than silently switching to Ollama, and never interrupts startup or payment processing.
4. **Authoritative Hugging Face Model Identity (`crates/esa-agents/src/provider.rs`, `hf_zerogpu.rs`, `deploy/huggingface-zerogpu/app.py`):**
   - Pinned `HF_AUTHORITATIVE_MODEL_ID = "Qwen/Qwen2.5-0.5B-Instruct"` and `HF_AUTHORITATIVE_MODEL_REVISION = "7ae557604adf67be50417f59c2c2f167def9a775"`.
   - Mismatched `HF_AI_MODEL_ID` or per-request `model_id` overrides are rejected with `InferenceErrorCategory::Misconfigured`; telemetry always records `HF_AUTHORITATIVE_MODEL_ID`.
5. **ZeroGPU Runtime Compatibility (`deploy/huggingface-zerogpu/requirements.txt`, `app.py`, `README.md`):**
   - Removed `torch>=2.3.0` from `requirements.txt` so the ZeroGPU Space uses the platform-supported PyTorch (`>=2.8.0`) without duplicate PyTorch installation.
   - Kept `transformers>=4.45.0,<5.0.0` and `accelerate>=0.34.0,<2.0.0` with Gradio `5.20.0`.
   - Replaced `device_map="auto"` with module-level `.to("cuda")` placement in `app.py` (and `.to("cuda")` on tokenized inputs) per Hugging Face ZeroGPU documentation.
6. **Hugging Face URL Safety (`crates/esa-agents/src/provider.rs`, `hf_zerogpu.rs`):**
   - Added `validate_hf_base_url(raw, allow_insecure_loopback_for_tests)`: enforces `https://` for all remote endpoints, rejects embedded credentials (`user@host`), query parameters (`?`), fragments (`#`), whitespace/control chars, and malformed hosts/ports. Insecure `http://127.0.0.1` / `http://localhost` is allowed only when `allow_insecure_loopback_for_tests` is `true` in unit tests.

---

## 2. Tests Added

- **Scope & Cache Isolation (`hf_zerogpu.rs`):** `test_strict_execution_scope_validation_and_cache_isolation` (System, Merchant A, Merchant B, empty/whitespace/delimiter merchant IDs rejected, invalid scope never reads global cache or records global telemetry, `OllamaProvider` rejects invalid scope).
- **Config Secret Redaction & Strict Env Validation (`hf_zerogpu.rs`):** `test_ai_provider_config_debug_redacts_token_and_strict_env_validation` (fake token redacted in `Debug`, absent/`ollama`/`hf_zerogpu`, invalid provider string, missing URL, missing token, mismatched `HF_AI_MODEL_ID`).
- **URL Safety (`hf_zerogpu.rs`):** `test_hf_base_url_safety_validation` (`https://` remote ok, `http://` remote rejected, `http://127.0.0.1` rejected in production mode & allowed in test loopback mode, embedded credentials/query/fragment/malformed rejected).

---

## 3. Remaining Limitations & Deployment Requirements

- Live Space startup and CUDA execution on Hugging Face ZeroGPU hardware have not been executed locally (local checks used `ESA_HF_MOCK_MODE=true` and mocked HTTP/SSE servers).
- Deployment requires a private Hugging Face Space with Gradio `5.20.0`, ZeroGPU-provided PyTorch `>=2.8.0`, and backend-only `HF_AI_BRAIN_TOKEN` (`sync: false` in `render.yaml`).

