#!/bin/sh
set -e

echo "🚀 Starting ESA Ollama Cloud Brain on port 7860..."
/bin/ollama serve &
OLLAMA_PID=$!

cleanup() {
    echo "🛑 Shutting down Ollama server..."
    kill -TERM "$OLLAMA_PID" 2>/dev/null || true
    wait "$OLLAMA_PID"
    exit 0
}
trap cleanup TERM INT

echo "⏳ Waiting for Ollama API to be reachable..."
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

echo "🔥 Pre-warming 'qwen2.5:0.5b' into memory (keep_alive: -1)..."
curl -s -X POST http://localhost:7860/api/generate \
    -H "Content-Type: application/json" \
    -d '{"model": "qwen2.5:0.5b", "prompt": "", "keep_alive": -1}' > /dev/null 2>&1 || true

echo "=================================================="
echo "🎉 ESA Ollama Cloud Brain is online & warm!"
echo "📡 Listening on http://0.0.0.0:7860"
echo "=================================================="

wait "$OLLAMA_PID"
