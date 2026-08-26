#!/bin/bash
set -e

echo "🚀 Starting ESA Payment Gateway (Quick Mode)"
echo "============================================="

# Start only Redis (already downloaded)
echo "📦 Starting Redis..."
docker run -d --name esa-redis -p 6379:6379 redis:7-alpine 2>/dev/null || docker start esa-redis 2>/dev/null || true

sleep 2

# Set environment for standalone mode (no Postgres, no Ollama needed)
export DATABASE_URL="memory"
export REDIS_URL="redis://localhost:6379"
export NATS_URL="memory"
export OLLAMA_URL="http://localhost:11434"
export API_HOST="0.0.0.0"
export API_PORT=8080
export RUST_LOG=info

echo "🦀 Building Rust backend..."
cargo build --release --bin esa-api

echo "🚀 Starting backend API on http://localhost:8080..."
cargo run --release --bin esa-api &
BACKEND_PID=$!

# Wait for backend to be ready
echo "⏳ Waiting for backend to start..."
for i in {1..30}; do
    if curl -s http://localhost:8080/health > /dev/null 2>&1; then
        echo "✅ Backend is ready!"
        break
    fi
    sleep 1
done

echo "🎨 Starting frontend on http://localhost:3000..."
cd frontend
bun run dev &
FRONTEND_PID=$!

sleep 3

echo ""
echo "✅ ESA Payment Gateway is running!"
echo "=================================="
echo ""
echo "🌐 Frontend: http://localhost:3000"
echo "🔧 Backend API: http://localhost:8080"
echo "📊 Health Check: http://localhost:8080/health"
echo "📈 Metrics: http://localhost:8080/metrics"
echo ""
echo "🎯 Next steps:"
echo "   1. Open http://localhost:3000 in your browser"
echo "   2. Click 'Trigger Traffic Spike' on the Dashboard"
echo "   3. Watch the autonomous recovery!"
echo ""
echo "To stop: Press Ctrl+C, then run:"
echo "   kill $BACKEND_PID $FRONTEND_PID"
echo "   docker stop esa-redis"
echo ""

# Keep script running
wait
