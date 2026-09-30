---
title: ESA Ollama Cloud Brain
emoji: ⚡
colorFrom: indigo
colorTo: blue
sdk: docker
app_port: 7860
pinned: false
---

# ⚡ ESA Ollama Autonomous Brain (24/7 Cloud Inference)

This Hugging Face Space hosts the dedicated 24/7 AI reasoning engine for the **ESA (Executable State Architecture) Multi-Gateway Payment Resilience Platform**.

- **RAM**: 16 GB Cloud Container
- **Port**: 7860
- **Pre-warmed Model**: `qwen2.5:0.5b`
- **Zero Local GPU Overhead**: 100% cloud-hosted.

### Endpoints
- **Health Check / Tags**: `GET https://sujithputta-esa-ollama-brain.hf.space/api/tags`
- **Generate**: `POST https://sujithputta-esa-ollama-brain.hf.space/api/generate`
- **Chat**: `POST https://sujithputta-esa-ollama-brain.hf.space/api/chat`
