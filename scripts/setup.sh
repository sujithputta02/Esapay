#!/bin/bash
set -e

echo "🚀 ESA Setup Script"
echo "===================="

# Check if running on macOS
if [[ "$OSTYPE" != "darwin"* ]]; then
    echo "⚠️  This script is optimized for macOS. Continue anyway? (y/n)"
    read -r response
    if [[ "$response" != "y" ]]; then
        exit 1
    fi
fi

# Check for required tools
echo "📦 Checking dependencies..."

command -v cargo >/dev/null 2>&1 || { echo "❌ Rust not found. Install from https://rustup.rs"; exit 1; }
command -v bun >/dev/null 2>&1 || { echo "❌ Bun not found. Install from https://bun.sh"; exit 1; }
command -v docker >/dev/null 2>&1 || { echo "❌ Docker not found. Install from https://docker.com"; exit 1; }
command -v docker-compose >/dev/null 2>&1 || { echo "❌ Docker Compose not found. Install Docker Desktop"; exit 1; }

echo "✅ All dependencies found"

# Check for Ollama
if command -v ollama >/dev/null 2>&1; then
    echo "✅ Ollama found"
else
    echo "⚠️  Ollama not found. Install it? (y/n)"
    read -r response
    if [[ "$response" == "y" ]]; then
        curl -fsSL https://ollama.com/install.sh | sh
    else
        echo "⚠️  Ollama required for AI agents. Install manually from https://ollama.com"
    fi
fi

# Pull Ollama model
echo "📥 Pulling Ollama model..."
ollama pull qwen2.5:0.5b || echo "⚠️  Failed to pull model. Continue with manual pull."

# Copy environment file
if [ ! -f .env ]; then
    echo "📝 Creating .env file..."
    cp .env.example .env
    echo "✅ Created .env file"
else
    echo "ℹ️  .env file already exists"
fi

# Install frontend dependencies
echo "📦 Installing frontend dependencies..."
cd frontend
bun install
cd ..

# Start infrastructure
echo "🐳 Starting infrastructure services..."
docker-compose up -d postgres redis nats ollama

# Wait for services
echo "⏳ Waiting for services to be healthy..."
sleep 10

# Check service health
docker-compose ps

echo ""
echo "✅ Setup complete!"
echo ""
echo "📚 Next steps:"
echo "1. Start backend:  cargo run --bin esa-api"
echo "2. Start frontend: cd frontend && bun run dev"
echo "3. Open browser:   http://localhost:3000"
echo ""
echo "🎯 To trigger demo:"
echo "   Click 'Trigger Spike' button on dashboard"
echo ""
