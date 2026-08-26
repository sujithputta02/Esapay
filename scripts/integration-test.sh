#!/bin/bash
set -e

echo "🔗 ESA Integration Test"
echo "======================="

# Start infrastructure
echo "🐳 Starting infrastructure..."
docker-compose up -d postgres redis nats ollama
sleep 10

# Check services are healthy
echo "✓ Checking service health..."
docker-compose ps

# Set environment
export DATABASE_URL="postgres://esa:esa_dev_password@localhost:5432/esa_db"
export REDIS_URL="redis://localhost:6379"
export NATS_URL="nats://localhost:4222"
export OLLAMA_URL="http://localhost:11434"
export OLLAMA_MODEL="qwen2.5:0.5b"
export RUST_LOG=debug

# Run backend in background
echo "🦀 Starting backend..."
cargo run --bin esa-api &
BACKEND_PID=$!

# Wait for backend to be ready
echo "⏳ Waiting for backend to start..."
sleep 15

# Test health endpoint
echo "🏥 Testing health endpoint..."
curl -f http://localhost:8080/health || { echo "❌ Health check failed"; kill $BACKEND_PID; exit 1; }
echo "✅ Health check passed"

# Test workloads endpoint
echo "📊 Testing workloads endpoint..."
curl -f http://localhost:8080/api/workloads || { echo "❌ Workloads endpoint failed"; kill $BACKEND_PID; exit 1; }
echo "✅ Workloads endpoint passed"

# Test token metrics endpoint
echo "🎫 Testing token metrics endpoint..."
curl -f http://localhost:8080/api/metrics/tokens || { echo "❌ Metrics endpoint failed"; kill $BACKEND_PID; exit 1; }
echo "✅ Token metrics endpoint passed"

# Stop backend
echo "🛑 Stopping backend..."
kill $BACKEND_PID

# Stop infrastructure
echo "🐳 Stopping infrastructure..."
docker-compose stop

echo ""
echo "✅ Integration tests passed!"
