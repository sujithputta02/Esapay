#!/bin/sh
set -e

# ==============================================================================
# ESA Payment Gateway - Ollama 24/7 Live Container Entrypoint
# ==============================================================================
# Ensures Ollama server runs continuously and all required models are
# automatically downloaded, pre-warmed, and kept permanently in memory.
# ==============================================================================

echo "🚀 Starting Ollama daemon in background..."
/bin/ollama serve &
OLLAMA_PID=$!

# Trap termination signals to gracefully shut down the server
cleanup() {
    echo "🛑 Shutting down Ollama server (PID: $OLLAMA_PID)..."
    kill -TERM "$OLLAMA_PID" 2>/dev/null || true
    wait "$OLLAMA_PID"
    exit 0
}
trap cleanup TERM INT

# Wait for Ollama server to become available
echo "⏳ Waiting for Ollama server API to be reachable..."
MAX_RETRIES=30
RETRY_COUNT=0
until /bin/ollama list > /dev/null 2>&1; do
    RETRY_COUNT=$((RETRY_COUNT + 1))
    if [ "$RETRY_COUNT" -ge "$MAX_RETRIES" ]; then
        echo "❌ Ollama server failed to start after $MAX_RETRIES attempts."
        exit 1
    fi
    sleep 1
done

echo "✅ Ollama API is active!"

# Determine which models to ensure are loaded
# Accepts OLLAMA_MODELS (space separated) or falls back to OLLAMA_MODEL
TARGET_MODELS="${OLLAMA_MODELS:-${OLLAMA_MODEL:-qwen2.5:0.5b}}"

echo "📦 Ensuring models are pulled and ready: $TARGET_MODELS"

for MODEL in $TARGET_MODELS; do
    echo "--------------------------------------------------"
    echo "🔍 Checking model: $MODEL"
    
    # Check if model is already pulled in the persistent volume
    if ! /bin/ollama list | grep -q "^${MODEL}[[:space:]]"; then
        echo "📥 Model '$MODEL' not found in cache. Pulling now..."
        /bin/ollama pull "$MODEL"
        echo "✅ Model '$MODEL' pulled successfully!"
    else
        echo "⚡ Model '$MODEL' is already cached."
    fi

    # Pre-warm model into memory with infinite keep-alive (-1)
    echo "🔥 Pre-warming '$MODEL' into memory (24/7 keep-alive)..."
    curl -s -X POST http://localhost:11434/api/generate \
        -H "Content-Type: application/json" \
        -d "{\"model\": \"$MODEL\", \"prompt\": \"\", \"keep_alive\": -1}" > /dev/null 2>&1 || true
    echo "✅ '$MODEL' is warm and live in memory!"
done

echo "=================================================="
echo "🎉 All Ollama models are online, pre-warmed & LIVE 24/7!"
echo "📡 Listening on http://0.0.0.0:11434"
echo "=================================================="

# Wait indefinitely for the Ollama background process
wait "$OLLAMA_PID"
