#!/bin/bash
set -e

echo "🚀 Deploy ESA Ollama Cloud Brain to Hugging Face Spaces"
echo "======================================================"

if [ -z "$1" ]; then
    echo "Usage: ./scripts/deploy-hf-ollama.sh <your-hf-username> [space-name]"
    echo "Example: ./scripts/deploy-hf-ollama.sh sujithputta esa-ollama-brain"
    exit 1
fi

HF_USER="$1"
SPACE_NAME="${2:-esa-ollama-brain}"
TARGET_DIR="/tmp/hf-space-${SPACE_NAME}"

echo "📦 Preparing Hugging Face Space repository: ${HF_USER}/${SPACE_NAME}..."

rm -rf "$TARGET_DIR"
git clone "https://huggingface.co/spaces/${HF_USER}/${SPACE_NAME}" "$TARGET_DIR" || {
    echo "⚠️ Space might not exist yet. Please create a new Space at https://huggingface.co/new-space"
    echo "   - SDK: Docker"
    echo "   - Visibility: Public"
    echo "   - Hardware: Free (16GB RAM)"
    exit 1
}

echo "📋 Copying container definition files..."
cp deploy/huggingface-ollama/README.md "$TARGET_DIR/"
cp deploy/huggingface-ollama/Dockerfile "$TARGET_DIR/"
cp deploy/huggingface-ollama/entrypoint.sh "$TARGET_DIR/"

cd "$TARGET_DIR"
git add .
git commit -m "Deploy ESA 24/7 Ollama Cloud Brain (16GB RAM)" || true
git push origin main

echo ""
echo "✅ Pushed successfully to Hugging Face Spaces!"
echo "🌐 Live URL will be: https://${HF_USER}-${SPACE_NAME}.hf.space"
echo "📡 You can set this as OLLAMA_URL in Railway: https://${HF_USER}-${SPACE_NAME}.hf.space"
echo ""
