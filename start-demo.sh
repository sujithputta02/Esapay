#!/bin/bash

echo "🚀 Starting ESA Razorpay Demo..."
echo ""
echo "This will start three services:"
echo "  1. Backend API (Rust) - Port 8080"
echo "  2. Payment Simulator (Next.js) - Port 3000"
echo "  3. ESA Control Plane (React) - Port 3001"
echo ""
echo "Press Ctrl+C in each terminal to stop"
echo ""

# Check if Ollama is running (local or Docker)
OLLAMA_HOST_URL="${OLLAMA_URL:-http://localhost:11434}"
if curl -s -f "${OLLAMA_HOST_URL}/api/tags" > /dev/null 2>&1; then
    echo "✅ Ollama is live & reachable at ${OLLAMA_HOST_URL}"
else
    echo "⚠️  Ollama is not responding at ${OLLAMA_HOST_URL}!"
    if command -v docker > /dev/null 2>&1 && docker info > /dev/null 2>&1; then
        echo "🐳 Starting 24/7 live Dockerized Ollama..."
        docker-compose up -d ollama
        echo "⏳ Waiting for Dockerized Ollama to finish warming models..."
        sleep 5
    else
        echo "   👉 Start it via Docker: docker-compose up -d ollama"
        echo "   👉 Or natively: ollama serve"
        echo ""
    fi
fi

# Check if model is available in Ollama
CHECK_MODEL="${OLLAMA_MODEL:-llama3.2:1b}"
if curl -s "${OLLAMA_HOST_URL}/api/tags" | grep -q "\"${CHECK_MODEL}\""; then
    echo "✅ Model '${CHECK_MODEL}' is active & kept live 24/7"
else
    echo "ℹ️  Model '${CHECK_MODEL}' will be auto-pulled on first agent deliberation"
fi

echo "Opening three terminal windows..."
echo ""

# For macOS
if [[ "$OSTYPE" == "darwin"* ]]; then
    # Terminal 1: Backend
    osascript -e 'tell application "Terminal" to do script "cd '"$(pwd)"' && echo \"🦀 Starting Backend API (Rust)...\" && cargo run --release --bin esa-api"'
    sleep 2
    
    # Terminal 2: Payment Simulator
    osascript -e 'tell application "Terminal" to do script "cd '"$(pwd)"'/payment-simulator && echo \"💳 Starting Payment Simulator (Next.js)...\" && bun run dev"'
    sleep 2
    
    # Terminal 3: ESA Frontend
    osascript -e 'tell application "Terminal" to do script "cd '"$(pwd)"'/frontend && echo \"🎛️  Starting ESA Control Plane (React)...\" && bun run dev"'
    
    echo "✅ All terminals opened!"
    echo ""
    echo "Wait ~30 seconds for everything to start, then:"
    echo ""
    echo "  📱 Payment Simulator:  http://localhost:3000"
    echo "  🎛️  ESA Control Plane:  http://localhost:3001"
    echo "  🔧 Backend API:        http://localhost:8080"
    echo ""
    echo "📖 Read docs/DEMO_GUIDE.md for the complete demo script!"
else
    echo "Manual start required (non-macOS):"
    echo ""
    echo "Terminal 1:"
    echo "  cd $(pwd) && cargo run --release --bin esa-api"
    echo ""
    echo "Terminal 2:"
    echo "  cd $(pwd)/payment-simulator && bun run dev"
    echo ""
    echo "Terminal 3:"
    echo "  cd $(pwd)/frontend && bun run dev"
fi
