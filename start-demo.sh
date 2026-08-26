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

# Check if Ollama is running
if ! pgrep -f ollama > /dev/null; then
    echo "⚠️  Warning: Ollama is not running!"
    echo "   Start it with: ollama serve"
    echo ""
fi

# Check if required model is available
if ! ollama list | grep -q "llama3.2:1b"; then
    echo "⚠️  Warning: llama3.2:1b model not found!"
    echo "   Pull it with: ollama pull llama3.2:1b"
    echo ""
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
