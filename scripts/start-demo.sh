#!/bin/bash
set -e

echo "🎬 Starting ESA Demo"
echo "==================="

# Start infrastructure if not running
echo "🐳 Checking infrastructure..."
docker-compose up -d postgres redis nats ollama prometheus grafana

# Wait for services
sleep 5

# Start backend in background
echo "🦀 Starting Rust backend..."
export DATABASE_URL="postgres://esa:esa_dev_password@localhost:5432/esa_db"
export REDIS_URL="redis://localhost:6379"
export NATS_URL="nats://localhost:4222"
export OLLAMA_URL="http://localhost:11434"
export OLLAMA_MODEL="qwen2.5:0.5b"
export RUST_LOG=info

cargo run --bin esa-api &
BACKEND_PID=$!

# Wait for backend to start
echo "⏳ Waiting for backend..."
sleep 10

# Start frontend in background
echo "⚛️  Starting React frontend..."
cd frontend
bun run dev &
FRONTEND_PID=$!

cd ..

echo ""
echo "✅ ESA Demo is running!"
echo ""
echo "🌐 Access Points:"
echo "   Frontend:   http://localhost:3000"
echo "   API:        http://localhost:8080"
echo "   Prometheus: http://localhost:9090"
echo "   Grafana:    http://localhost:3001 (admin/admin)"
echo ""
echo "🎯 Demo Instructions:"
echo "   1. Open http://localhost:3000"
echo "   2. Click 'Trigger Spike' on dashboard"
echo "   3. Watch autonomous recovery in real-time"
echo "   4. Check Agents page for AI activity"
echo "   5. View Audit page for decision lineage"
echo ""
echo "🛑 To stop: Press Ctrl+C"
echo ""

# Wait for Ctrl+C
trap "echo ''; echo '🛑 Stopping demo...'; kill $BACKEND_PID $FRONTEND_PID; docker-compose stop; exit 0" INT

wait
