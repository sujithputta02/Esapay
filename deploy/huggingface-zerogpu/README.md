---
title: ESA AI Brain
emoji: ⚡
colorFrom: indigo
colorTo: blue
sdk: gradio
sdk_version: 5.20.0
app_file: app.py
pinned: false
---

# ⚡ ESA AI Brain (Experimental ZeroGPU Inference)

This private Hugging Face Space provides experimental remote inference capacity for **ESAPay** via **Hugging Face ZeroGPU** and **Gradio 5.20.0**.

- **Compute Model**: Hugging Face ZeroGPU (shared, dynamically allocated GPU time subject to account quota and queue wait times — not a dedicated A10G or 24/7 guaranteed endpoint)
- **Runtime**: Gradio `5.20.0` with ZeroGPU-provided PyTorch (`>=2.8.0`) and module-level `.to("cuda")` placement
- **Model**: `Qwen/Qwen2.5-0.5B-Instruct`
- **Pinned Revision**: `7ae557604adf67be50417f59c2c2f167def9a775`
- **License**: Apache License 2.0 (`apache-2.0`)
- **Authority**: Advisory text generation only; Rust `OutputValidator`, privacy redaction, policy engine, and deterministic fallback remain authoritative in ESAPay.

### Gradio Queue API Contract

This Space exposes the Gradio endpoint `generate` (it does **not** expose Ollama `/api/generate`, `/api/chat`, or `/api/tags` endpoints):

1. **Submit inference job**
   - `POST /gradio_api/call/generate`
   - Headers: `Authorization: Bearer <HF_AI_BRAIN_TOKEN>`, `Content-Type: application/json`
   - Body: `{"data": ["PROMPT_TEXT"]}`
   - Response: `{"event_id": "..."}`
2. **Stream inference result (SSE)**
   - `GET /gradio_api/call/generate/{event_id}`
   - Headers: `Authorization: Bearer <HF_AI_BRAIN_TOKEN>`, `Accept: text/event-stream`
   - Events: `status`, `progress`, `complete` (`data: ["..."]`), `error`
