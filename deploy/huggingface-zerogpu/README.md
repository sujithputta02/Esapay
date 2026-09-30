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

# ⚡ ESA AI Autonomous Brain (ZeroGPU NVIDIA A10G)

This Hugging Face Space hosts the 24/7 dedicated AI reasoning engine for the **ESA (Executable State Architecture) Multi-Gateway Payment Resilience Platform**.

- **Hardware**: NVIDIA A10G (24GB VRAM) via Hugging Face ZeroGPU
- **Model**: `Qwen/Qwen2.5-0.5B-Instruct`
- **Cost**: 100% Free Tier (ZeroGPU)
- **Zero Local GPU Overhead**: Runs in the cloud without battery or thermal load on your local machine.

### Endpoints
- **Web Interface**: `https://sujithputta-esa-ai-brain.hf.space`
- **Health Check**: `GET https://sujithputta-esa-ai-brain.hf.space/health`
- **Model Tags (Ollama Compatible)**: `GET https://sujithputta-esa-ai-brain.hf.space/api/tags`
- **Generate (Ollama Compatible)**: `POST https://sujithputta-esa-ai-brain.hf.space/api/generate`
- **Chat (Ollama Compatible)**: `POST https://sujithputta-esa-ai-brain.hf.space/api/chat`
